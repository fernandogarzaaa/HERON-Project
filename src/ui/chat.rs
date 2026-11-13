//! Chat view model for the HERON Slint rebuild. Chat lane.
//!
//! This file owns the thread model and the state machine that applies
//! SSE events to it. It is std-only on purpose. It does not touch the
//! network, the token, or the Slint bindings. The Shell lane owns the
//! bridge. The bridge converts server JSON into the `ChatEvent` values
//! here and calls `ChatView::apply_event` in arrival order.
//!
//! Markdown belongs to Slint, not to this file. Replies render with
//! `slint::StyledText::from_markdown`, per the plan in
//! `~/workspace/HERON-Slint-Plan.md`. While a reply streams, the view
//! shows plain text. Markdown is rendered once, at `done`. This file
//! therefore keeps the raw reply text and never parses it. An earlier
//! draft of this file had its own markdown parser. That parser is
//! deleted. It duplicated StyledText and re-parsed the whole reply on
//! every delta, which the plan names as the sharpest risk.
//!
//! The `.slint` model type is `ChatMsg` (see
//! `ui/components/message.slint`). The Rust side keeps `ThreadItem`.
//! The binding maps them: `User(text)` becomes a ChatMsg with role
//! "user", body `from_plain_text(text)`, text the text; `Turn`
//! becomes a ChatMsg with role "assistant", body
//! `from_plain_text(text)` while streaming and
//! `from_markdown(text)` at done, text the raw reply text for the
//! Copy button, plus its steps, approvals, notes, failure,
//! complete, and live flags; `Note(text)` becomes a ChatMsg with role
//! "note", body `from_plain_text(text)`, text the text. The bridge
//! must not re-parse the whole reply on every `text` delta: that is
//! O(n) per chunk and the plan's sharpest risk.
//!
//! The `ChatEvent` here is the normalized shape the bridge converts
//! the shell's `ChatEvent` (src/bin/desktop/bridge.rs) into. The two
//! match field for field except `Learned`: the shell carries
//! `Vec<serde_json::Value>` and this file carries `Vec<String>`, so
//! the bridge stringifies each item when it converts. `always_rule`
//! is `Option<String>` on both sides: None means the server sent no
//! rule, and the always button falls back to the generic label.
//!
//! Every behaviour here is traced to `assets/index.html` or
//! `src/server.rs`. See `ui/CHAT_MAP.md` for the full map.
//!
//! Verify with: `rustc --edition 2021 --test src/ui/chat.rs -o /tmp/chat_test && /tmp/chat_test`

use std::collections::{HashMap, HashSet};

/// A tool call row in the track. Status mirrors the web classes:
/// running, ok, error. Field names follow the plan's `ToolStep`
/// (`id, name, summary, risk, state, preview`) plus the label the web
/// client derives and the expanded flag the view owns.
#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Running,
    Ok,
    Error,
}

impl StepStatus {
    /// Wire value for the `.slint` `state` field.
    pub fn wire(self) -> &'static str {
        match self {
            StepStatus::Running => "run",
            StepStatus::Ok => "ok",
            StepStatus::Error => "err",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolStep {
    pub id: String,
    pub name: String,
    pub label: String,
    pub summary: String,
    pub risk: String,
    pub status: StepStatus,
    /// Result text, cut to 1500 characters like the web client.
    pub preview: String,
    /// Error rows start open. Other rows start closed.
    pub expanded: bool,
}

/// One inline approval card.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalCard {
    pub id: String,
    pub tool: String,
    pub title: String,
    pub warning: String,
    pub detail: String,
    /// Red styling. True for DANGEROUS or sensitive risk, except the
    /// three standing change tools, which stay amber.
    pub danger: bool,
    pub can_session: bool,
    pub can_always: bool,
    pub session_label: String,
    pub always_label: String,
    /// Set once answered. The card is then replaced by this note.
    pub decision_note: Option<String>,
}

/// One assistant turn: raw reply text, tool track, approval cards,
/// notes. `raw_text` is shown as plain text while `live`, and rendered
/// as markdown by the `.slint` side once the turn completes.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    /// Monotonic stream identity; the forwarder must retain this id.
    pub turn_id: u64,
    pub raw_text: String,
    pub steps: Vec<ToolStep>,
    pub approvals: Vec<ApprovalCard>,
    pub notes: Vec<String>,
    pub failure: Option<String>,
    pub complete: bool,
    /// True while the reply is still streaming.
    pub live: bool,
}

/// One row of the thread.
#[derive(Debug, Clone)]
pub enum ThreadItem {
    User(String),
    Turn(Turn),
    Note(String),
}

/// Permission mode. Names match `src/permissions.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ReadOnly,
    Ask,
    Auto,
    Yolo,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "readonly" | "read-only" | "ro" => Some(Mode::ReadOnly),
            "ask" => Some(Mode::Ask),
            "auto" => Some(Mode::Auto),
            "yolo" => Some(Mode::Yolo),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::ReadOnly => "readonly",
            Mode::Ask => "ask",
            Mode::Auto => "auto",
            Mode::Yolo => "yolo",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::ReadOnly => "Read only",
            Mode::Ask => "Ask first",
            Mode::Auto => "Auto",
            Mode::Yolo => "Unrestricted",
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Ask
    }
}

/// Approval decisions, matching `POST /api/approve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Once,
    Session,
    Always,
    Deny,
}

impl ApprovalDecision {
    pub fn wire(self) -> &'static str {
        match self {
            ApprovalDecision::Once => "once",
            ApprovalDecision::Session => "session",
            ApprovalDecision::Always => "always",
            ApprovalDecision::Deny => "deny",
        }
    }
}

/// One SSE event from `POST /chat`, after the bridge parses the JSON.
/// Field names match the server shapes in `src/server.rs`.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    Session { id: String },
    Text { text: String },
    ToolStart { id: String, name: String, summary: String, risk: String },
    ToolEnd { id: String, name: String, ok: bool, preview: String },
    Approval {
        id: String,
        tool: String,
        summary: String,
        risk: String,
        detail: String,
        always_rule: Option<String>,
        can_always: bool,
        can_session: bool,
        tainted: bool,
    },
    Notice { text: String },
    Model { name: String, why: String },
    Learned { items: Vec<String> },
    /// The `usage` event carries token counts. The web client shows
    /// none of it in the thread. The view ignores it.
    Usage { prompt: u64, completion: u64 },
    Error { message: String },
    Cancelled,
    Done { text: String, title: String },
}

/// One stored message, as returned by `GET /api/sessions/{id}`.
/// Mirrors `src/llm.rs` `Message`. The bridge converts server JSON
/// into this shape.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Vec<HistoryToolCall>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Fixed tool label map from `assets/index.html`.
pub fn tool_label(name: &str) -> String {
    let label = match name {
        "shell" => "Ran",
        "read_file" => "Read",
        "write_file" => "Wrote",
        "edit_file" => "Edited",
        "list_dir" => "Listed",
        "search_files" => "Searched files for",
        "find_files" => "Looked for files",
        "web_fetch" => "Opened",
        "browser_open" => "Opened in the browser",
        "browser_read" => "Looked at the page",
        "browser_click" => "In the browser:",
        "browser_type" => "In the browser:",
        "web_search" => "Searched the web for",
        "remember" => "Remembered",
        "recall" => "Checked memory for",
        "forget" => "Forgot memory",
        "task_add" => "Added task",
        "task_list" => "Checked tasks",
        "task_update" => "Updated task",
        "schedule_add" => "Scheduled",
        "schedule_list" => "Checked reminders",
        "schedule_remove" => "Cancelled reminder",
        "use_skill" => "Loaded skill",
        "delegate" => "Handed off",
        "save_automation" => "Set up automation",
        "update_persona" => "Changed personality",
        "save_skill" => "Saved skill",
        "search_knowledge" => "Searched your email, calendar and documents for",
        "if" => "Checked a condition",
        "notify" => "Told you",
        "ask" => "Asked the AI model",
        "agent" => "Worked on it",
        "for_each" => "Repeated for each item",
        "set" => "Worked out a value",
        "stop" => "Stopped",
        _ => return clean_tool_name(name),
    };
    label.to_string()
}

/// Strip a leading `mcp__` and turn `__` into ` → `.
pub fn clean_tool_name(name: &str) -> String {
    name.strip_prefix("mcp__").unwrap_or(name).replace("__", " → ")
}

/// Approval card title, from `assets/index.html`.
pub fn ask_title(tool: &str) -> String {
    match tool {
        "shell" => "Heron wants to run a command".to_string(),
        "write_file" => "Heron wants to write a file".to_string(),
        "edit_file" => "Heron wants to edit a file".to_string(),
        "read_file" => "Heron wants to read a sensitive file".to_string(),
        "list_dir" => "Heron wants to open a sensitive folder".to_string(),
        "search_files" => "Heron wants to search a sensitive folder".to_string(),
        "save_automation" => "Heron wants to set up an automation".to_string(),
        "update_persona" => "Heron wants to change its personality".to_string(),
        "save_skill" => "Heron wants to save a skill for next time".to_string(),
        _ => format!("Heron wants to use {}", clean_tool_name(tool)),
    }
}

/// True for the three standing change tools. They always need approval
/// and they never take the red danger styling.
pub fn is_standing_change(tool: &str) -> bool {
    matches!(tool, "save_automation" | "update_persona" | "save_skill")
}

/// Warning line for an approval card. Empty when none applies.
pub fn approval_warning(tool: &str, risk: &str, tainted: bool) -> String {
    if is_standing_change(tool) {
        return "This changes what Heron does from now on, so it always needs your OK. Read it through before allowing.".to_string();
    }
    let mut warning = match risk {
        "DANGEROUS" => "This could delete data, reach outside your folder, or be hard to undo.",
        "sensitive" => "This reads something that may hold passwords, keys or other secrets, and would show it to your AI model.",
        "external" => "This acts on a website or another app. Heron cannot undo it.",
        "egress" => "This sends text Heron wrote to somewhere outside your computer. Check that nothing private is in it.",
        _ => "",
    }
    .to_string();
    if tainted {
        if !warning.is_empty() {
            warning.push(' ');
        }
        warning.push_str("Heron has read content from outside (a web page, an email or a connected app) in this conversation. Check that this is something you asked for.");
    }
    warning
}

/// Red styling for an approval card.
pub fn approval_danger(tool: &str, risk: &str) -> bool {
    !is_standing_change(tool) && matches!(risk, "DANGEROUS" | "sensitive")
}

/// Label for the "allow for this conversation" button.
pub fn session_button_label(tool: &str) -> String {
    if tool == "web_search" {
        "Allow searches for this conversation".to_string()
    } else {
        "Allow for this conversation".to_string()
    }
}

/// Label for the "always allow" button. `always_rule` comes from the
/// server. For shell it names the command pattern.
pub fn always_button_label(tool: &str, always_rule: &str) -> String {
    if tool == "shell" {
        let rule = always_rule
            .strip_prefix("shell:")
            .unwrap_or(always_rule)
            .trim_end_matches(" *")
            .trim()
            .to_string();
        format!("Always allow “{rule}” commands")
    } else {
        "Always allow this kind of action".to_string()
    }
}

/// Note shown after an approval is answered.
pub fn decision_note(decision: ApprovalDecision, tool: &str) -> String {
    match decision {
        ApprovalDecision::Once => "You allowed this.".to_string(),
        ApprovalDecision::Session => {
            if tool == "shell" {
                "You allowed this, and the same command for the rest of this conversation.".to_string()
            } else {
                "You allowed this, and the same kind of action for the rest of this conversation.".to_string()
            }
        }
        ApprovalDecision::Always => "You allowed this, and similar actions from now on.".to_string(),
        ApprovalDecision::Deny => "You didn't allow this.".to_string(),
    }
}

// ---------- small JSON field reader (std-only) ----------

/// Read a string field out of a flat JSON object. Returns None when the
/// field is missing, not a string, or the text is not an object. This
/// covers the tool argument shapes the server actually emits. It is not
/// a general JSON parser.
pub fn json_string_field(json: &str, key: &str) -> Option<String> {
    let bytes = json.as_bytes();
    let needle = format!("\"{key}\"");
    let mut start = 0;
    while let Some(pos) = json[start..].find(&needle) {
        let key_at = start + pos;
        let mut i = key_at + needle.len();
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b':' {
            start = key_at + 1;
            continue;
        }
        i += 1;
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'"' {
            start = key_at + 1;
            continue;
        }
        i += 1;
        let mut out = String::new();
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if i + 1 < bytes.len() => {
                    match bytes[i + 1] {
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'\\' => out.push('\\'),
                        b'"' => out.push('"'),
                        b'/' => out.push('/'),
                        b'u' if i + 5 < bytes.len() => {
                            let hex = &json[i + 2..i + 6];
                            if let Ok(n) = u32::from_str_radix(hex, 16) {
                                if let Some(c) = char::from_u32(n) {
                                    out.push(c);
                                }
                            }
                            i += 4;
                        }
                        other => out.push(other as char),
                    }
                    i += 2;
                }
                b'"' => return Some(out),
                _ => {
                    let ch_len = utf8_len(bytes[i]);
                    let end = (i + ch_len).min(bytes.len());
                    out.push_str(&json[i..end]);
                    i = end;
                }
            }
        }
        return None;
    }
    None
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// Read a raw (unquoted) field value, for numbers such as `id`.
fn json_raw_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let mut start = 0;
    while let Some(pos) = json[start..].find(&needle) {
        let key_at = start + pos;
        let bytes = json.as_bytes();
        let mut i = key_at + needle.len();
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b':' {
            start = key_at + 1;
            continue;
        }
        i += 1;
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        let value_start = i;
        while i < bytes.len() && !matches!(bytes[i], b',' | b'}' | b' ' | b'\n' | b'\t' | b'\r') {
            i += 1;
        }
        if value_start < i {
            return Some(&json[value_start..i]);
        }
        start = key_at + 1;
    }
    None
}

/// Argument summary for a tool step, from `assets/index.html`. The
/// arguments arrive as a JSON string. Cut to 160 characters.
pub fn arg_summary(args_json: &str) -> String {
    let pick = ["command", "path", "url", "query", "pattern", "glob", "task", "title", "content", "name"]
        .iter()
        .find_map(|k| json_string_field(args_json, k));
    let summary = match pick {
        Some(s) => s,
        None => {
            let when = json_string_field(args_json, "when");
            let prompt = json_string_field(args_json, "prompt");
            match (when, prompt) {
                (Some(w), Some(p)) => format!("{w} — {p}"),
                _ => match json_raw_field(args_json, "id") {
                    Some(id) => format!("#{id}"),
                    None => json_string_field(args_json, "id").map(|id| format!("#{id}")).unwrap_or_default(),
                },
            }
        }
    };
    summary.chars().take(160).collect()
}

// ---------- view state machine ----------

/// The chat view's state. The bridge drives it with `ChatEvent`
/// values. The Slint component reads the thread out.
#[derive(Debug, Default)]
pub struct ChatView {
    pub session_id: Option<String>,
    pub title: String,
    pub mode: Mode,
    pub busy: bool,
    pub thread: Vec<ThreadItem>,
    next_turn_id: u64,
    /// True unless the reader scrolled more than 240px from the bottom.
    pub following: bool,
    /// Expanded tool result rows, by step id.
    pub expanded_steps: HashSet<String>,
    pub can_undo: bool,
    pub has_key: bool,
    pub demo: bool,
    pub paused: bool,
    pub workspace: String,
    /// Label for the thinking indicator, when one is showing.
    pub thinking: Option<String>,
}

impl ChatView {
    pub fn new() -> Self {
        ChatView {
            title: "New conversation".to_string(),
            following: true,
            ..Default::default()
        }
    }

    /// Start a new conversation. Clears the thread and the session id.
    pub fn start_new(&mut self) {
        self.session_id = None;
        self.title = "New conversation".to_string();
        self.thread.clear();
        self.can_undo = false;
        self.busy = false;
        self.thinking = None;
    }

    /// Send a message. Adds the user bubble and an empty live turn.
    /// The bridge then opens the SSE stream.
    pub fn start_send(&mut self, text: &str) -> u64 {
        self.next_turn_id = self.next_turn_id.checked_add(1).expect("turn id overflow");
        let turn_id = self.next_turn_id;
        self.thread.push(ThreadItem::User(text.to_string()));
        self.thread.push(ThreadItem::Turn(Turn {
            turn_id,
            live: true,
            ..Default::default()
        }));
        self.busy = true;
        self.thinking = Some("Thinking…".to_string());
        self.following = true;
        turn_id
    }

    /// The live turn, if the last item is one.
    fn live_turn(&mut self) -> Option<&mut Turn> {
        match self.thread.last_mut() {
            Some(ThreadItem::Turn(t)) if t.live => Some(t),
            _ => None,
        }
    }

    /// A specific stream's live turn, if it still belongs to this view.
    fn live_turn_for(&mut self, turn_id: u64) -> Option<&mut Turn> {
        self.thread.iter_mut().find_map(|item| match item {
            ThreadItem::Turn(t) if t.turn_id == turn_id && t.live => Some(t),
            _ => None,
        })
    }

    /// Unlike live_turn_for, accepts a completed turn for late learning.
    fn turn_for(&mut self, turn_id: u64) -> Option<&mut Turn> {
        self.thread.iter_mut().find_map(|item| match item {
            ThreadItem::Turn(t) if t.turn_id == turn_id => Some(t),
            _ => None,
        })
    }

    /// Convenience for single-stream callers and unit tests. Integration
    /// forwarders MUST use apply_event_for_turn to preserve origin identity.
    pub fn apply_event(&mut self, ev: ChatEvent) {
        if let Some(turn_id) = self.thread.iter().rev().find_map(|item| match item {
            ThreadItem::Turn(t) if t.turn_id != 0 => Some(t.turn_id),
            _ => None,
        }) {
            self.apply_event_for_turn(turn_id, ev);
        }
    }

    /// Keep streamed events tied to the originating turn across Done and
    /// subsequent sends. A cleared thread cannot receive old events because
    /// its monotonically increasing turn id is never reused.
    pub fn apply_event_for_turn(&mut self, turn_id: u64, ev: ChatEvent) {
        let current = self.thread.iter().rev().find_map(|item| match item {
            ThreadItem::Turn(t) if t.turn_id != 0 => Some(t.turn_id),
            _ => None,
        }) == Some(turn_id);
        if self.turn_for(turn_id).is_none() {
            return;
        }
        match ev {
            ChatEvent::Session { id } => {
                if current {
                    self.session_id = Some(id);
                    self.can_undo = true;
                    self.thinking = Some("Thinking…".to_string());
                }
            }
            ChatEvent::Text { text } => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    turn.raw_text.push_str(&text);
                    if current { self.thinking = None; }
                }
            }
            ChatEvent::ToolStart { id, name, summary, risk } => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    turn.steps.push(ToolStep {
                        id,
                        label: tool_label(&name),
                        summary,
                        risk,
                        name,
                        status: StepStatus::Running,
                        preview: String::new(),
                        expanded: false,
                    });
                }
            }
            ChatEvent::ToolEnd { id, name, ok, preview } => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    if let Some(step) = turn.steps.iter_mut().find(|s| s.id == id) {
                        step.status = if ok { StepStatus::Ok } else { StepStatus::Error };
                        step.preview = preview.chars().take(1500).collect();
                        step.expanded = !ok;
                    } else {
                        turn.steps.push(ToolStep {
                            id,
                            label: tool_label(&name),
                            summary: String::new(),
                            risk: String::new(),
                            name,
                            status: if ok { StepStatus::Ok } else { StepStatus::Error },
                            preview: preview.chars().take(1500).collect(),
                            expanded: !ok,
                        });
                    }
                    if current { self.thinking = Some("Working out the next step…".to_string()); }
                }
            }
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
            } => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    let detail = if detail.is_empty() { summary } else { detail };
                    turn.approvals.push(ApprovalCard {
                        title: ask_title(&tool),
                        warning: approval_warning(&tool, &risk, tainted),
                        danger: approval_danger(&tool, &risk),
                        session_label: session_button_label(&tool),
                        always_label: always_button_label(&tool, always_rule.as_deref().unwrap_or("")),
                        id,
                        tool,
                        detail,
                        can_session,
                        can_always,
                        decision_note: None,
                    });
                }
            }
            ChatEvent::Notice { text } => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    turn.notes.push(text);
                    if current { self.thinking = Some("Thinking…".to_string()); }
                }
            }
            ChatEvent::Model { name, why } => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    turn.notes.push(format!("Using {name}: {why}."));
                }
            }
            ChatEvent::Learned { items } => {
                if let Some(turn) = self.turn_for(turn_id) {
                    turn.notes.push(format!("Saved to memory: {}", items.join(" / ")));
                }
            }
            ChatEvent::Usage { .. } => {
                // No thread display. The footer token count comes from
                // a state refresh.
            }
            ChatEvent::Error { message } => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    turn.failure = Some(message);
                    turn.live = false;
                    turn.complete = true;
                }
                if current {
                    self.busy = false;
                    self.thinking = None;
                }
            }
            ChatEvent::Cancelled => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    turn.notes.push("Stopped.".to_string());
                    turn.live = false;
                    turn.complete = true;
                }
                if current {
                    self.busy = false;
                    self.thinking = None;
                }
            }
            ChatEvent::Done { title, .. } => {
                if let Some(turn) = self.live_turn_for(turn_id) {
                    turn.live = false;
                    turn.complete = true;
                }
                if current && !title.is_empty() {
                    self.title = title;
                }
                if current {
                    self.busy = false;
                    self.thinking = None;
                }
            }
        }
    }

    /// The stream ended with no terminal event.
    pub fn stream_closed_early(&mut self) {
        if let Some(turn) = self.live_turn() {
            turn.notes
                .push("The connection to Heron closed before the reply finished.".to_string());
            turn.live = false;
            turn.complete = true;
        }
        self.busy = false;
        self.thinking = None;
    }

    /// A request failure before the first event. The failure block
    /// carries the server's `{ error }` text.
    pub fn request_failed(&mut self, message: &str) {
        if let Some(turn) = self.live_turn() {
            turn.failure = Some(message.to_string());
            turn.live = false;
            turn.complete = true;
        }
        self.busy = false;
        self.thinking = None;
    }

    /// Record an approval answer. The card is replaced by its decision
    /// note. Returns the note text. A failed approve call is reported
    /// by passing the error message as `err`.
    pub fn answer_approval(&mut self, id: &str, decision: ApprovalDecision, err: Option<&str>) -> Option<String> {
        for item in self.thread.iter_mut() {
            if let ThreadItem::Turn(turn) = item {
                if let Some(card) = turn.approvals.iter_mut().find(|c| c.id == id) {
                    let note = match err {
                        Some(e) => e.to_string(),
                        None => decision_note(decision, &card.tool),
                    };
                    card.decision_note = Some(note.clone());
                    return Some(note);
                }
            }
        }
        None
    }

    /// Toggle a tool result row open or closed.
    pub fn toggle_step(&mut self, id: &str) {
        for item in self.thread.iter_mut() {
            if let ThreadItem::Turn(turn) = item {
                if let Some(step) = turn.steps.iter_mut().find(|s| s.id == id) {
                    step.expanded = !step.expanded;
                    return;
                }
            }
        }
    }

    /// Replay stored history into the thread. Mirrors the web
    /// `renderHistory`. User messages starting with `[system notice]`
    /// are skipped. `interrupted` adds the cut short note.
    pub fn open_history(&mut self, messages: Vec<HistoryMessage>, interrupted: bool) {
        self.thread.clear();
        let mut pending: HashMap<String, (usize, String)> = HashMap::new();
        let mut turn_open = false;
        for m in messages {
            match m.role.as_str() {
                "user" => {
                    let content = m.content.unwrap_or_default();
                    if content.starts_with("[system notice]") {
                        continue;
                    }
                    self.thread.push(ThreadItem::User(content));
                    turn_open = false;
                }
                "assistant" => {
                    if !turn_open {
                        self.thread.push(ThreadItem::Turn(Turn::default()));
                        turn_open = true;
                    }
                    let idx = self.thread.len() - 1;
                    if let Some(ThreadItem::Turn(turn)) = self.thread.get_mut(idx) {
                        if let Some(content) = m.content.clone() {
                            if !content.is_empty() {
                                turn.raw_text = content;
                            }
                        }
                        for c in &m.tool_calls {
                            turn.steps.push(ToolStep {
                                id: c.id.clone(),
                                label: tool_label(&c.name),
                                summary: arg_summary(&c.arguments),
                                risk: String::new(),
                                name: c.name.clone(),
                                status: StepStatus::Ok,
                                preview: String::new(),
                                expanded: false,
                            });
                            pending.insert(c.id.clone(), (idx, c.name.clone()));
                        }
                        if m.content.as_deref().map(|c| !c.is_empty()).unwrap_or(false) && m.tool_calls.is_empty() {
                            turn.complete = true;
                            turn_open = false;
                        }
                    }
                }
                "tool" => {
                    if let Some(call_id) = &m.tool_call_id {
                        if let Some((idx, _name)) = pending.get(call_id) {
                            let idx = *idx;
                            if let Some(ThreadItem::Turn(turn)) = self.thread.get_mut(idx) {
                                if let Some(step) = turn.steps.iter_mut().find(|s| s.id == *call_id) {
                                    let txt = m.content.clone().unwrap_or_default();
                                    let is_err = txt.starts_with("Error:") || txt.starts_with("Permission denied");
                                    step.status = if is_err { StepStatus::Error } else { StepStatus::Ok };
                                    step.preview = txt.chars().take(1500).collect();
                                    step.expanded = is_err;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if interrupted {
            self.thread.push(ThreadItem::Note(
                "This reply was cut short before Heron finished.".to_string(),
            ));
        }
        self.busy = false;
    }

    /// Short workspace path, as the empty state shows it. Two segments
    /// with a "…/" prefix when longer.
    pub fn workspace_short(&self) -> String {
        workspace_short(&self.workspace)
    }
}

/// Short form of a workspace path: two segments with a "…/" prefix when
/// longer, the full path when short, "your folder" when empty. The Shell
/// poll needs it without a ChatView, so it lives here as a free function.
pub fn workspace_short(path: &str) -> String {
    let full = path.trim_end_matches(['/', '\\']);
    let parts: Vec<&str> = full.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.len() > 2 {
        format!("…/{}", parts[parts.len() - 2..].join("/"))
    } else if full.is_empty() {
        "your folder".to_string()
    } else {
        full.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arg_summary_picks_first_field() {
        assert_eq!(arg_summary(r#"{"command": "ls -la", "path": "/tmp"}"#), "ls -la");
        assert_eq!(arg_summary(r#"{"path": "/tmp/foo"}"#), "/tmp/foo");
        assert_eq!(arg_summary(r#"{"when": "tomorrow 9:00", "prompt": "call the dentist"}"#), "tomorrow 9:00 — call the dentist");
        assert_eq!(arg_summary(r#"{"id": 42}"#), "#42");
        assert_eq!(arg_summary("not json"), "");
    }

    #[test]
    fn labels_and_titles() {
        assert_eq!(tool_label("shell"), "Ran");
        assert_eq!(tool_label("write_file"), "Wrote");
        assert_eq!(tool_label("mcp__github__search_repos"), "github → search_repos");
        assert_eq!(ask_title("shell"), "Heron wants to run a command");
        assert_eq!(ask_title("mcp__github__search_repos"), "Heron wants to use github → search_repos");
        assert_eq!(session_button_label("web_search"), "Allow searches for this conversation");
        assert_eq!(session_button_label("shell"), "Allow for this conversation");
        assert_eq!(always_button_label("shell", "shell:git *"), "Always allow “git” commands");
        assert_eq!(always_button_label("shell", "shell: git *"), "Always allow “git” commands");
        assert_eq!(always_button_label("write_file", ""), "Always allow this kind of action");
        assert_eq!(decision_note(ApprovalDecision::Once, "shell"), "You allowed this.");
        assert_eq!(
            decision_note(ApprovalDecision::Session, "shell"),
            "You allowed this, and the same command for the rest of this conversation."
        );
        assert_eq!(decision_note(ApprovalDecision::Deny, "shell"), "You didn't allow this.");
        assert_eq!(StepStatus::Running.wire(), "run");
        assert_eq!(StepStatus::Ok.wire(), "ok");
        assert_eq!(StepStatus::Error.wire(), "err");
    }

    #[test]
    fn approval_warnings() {
        assert!(approval_warning("save_automation", "DANGEROUS", false).contains("from now on"));
        assert!(!approval_danger("save_automation", "DANGEROUS"));
        assert!(approval_danger("shell", "DANGEROUS"));
        assert!(approval_danger("read_file", "sensitive"));
        assert!(!approval_danger("shell", "normal"));
        let w = approval_warning("shell", "normal", true);
        assert!(w.contains("read content from outside"));
    }

    #[test]
    fn event_flow_happy_path() {
        let mut view = ChatView::new();
        view.start_send("hi");
        assert!(view.busy);
        view.apply_event(ChatEvent::Session { id: "s1".into() });
        assert_eq!(view.session_id.as_deref(), Some("s1"));
        assert!(view.can_undo);
        view.apply_event(ChatEvent::Text { text: "Hello **there**".into() });
        // While streaming, the raw text accumulates and the turn stays
        // live. Nothing is parsed here. The .slint side shows plain
        // text until done.
        let ThreadItem::Turn(turn) = view.thread.last().unwrap() else {
            panic!("expected turn");
        };
        assert!(turn.live && !turn.complete);
        assert_eq!(turn.raw_text, "Hello **there**");
        view.apply_event(ChatEvent::ToolStart {
            id: "t1".into(),
            name: "shell".into(),
            summary: "ls".into(),
            risk: "normal".into(),
        });
        view.apply_event(ChatEvent::ToolEnd {
            id: "t1".into(),
            name: "shell".into(),
            ok: true,
            preview: "a.txt".into(),
        });
        view.apply_event(ChatEvent::Done { text: "Hello there".into(), title: "Greeting".into() });
        assert!(!view.busy);
        assert_eq!(view.title, "Greeting");
        let ThreadItem::Turn(turn) = view.thread.last().unwrap() else {
            panic!("expected turn");
        };
        assert!(turn.complete && !turn.live);
        assert_eq!(turn.steps.len(), 1);
        assert_eq!(turn.steps[0].status, StepStatus::Ok);
        assert_eq!(turn.steps[0].label, "Ran");
        assert_eq!(turn.steps[0].risk, "normal");
    }

    #[test]
    fn learned_after_done_stays_with_origin_even_during_next_turn() {
        let mut view = ChatView::new();
        let first = view.start_send("first");
        view.apply_event_for_turn(first, ChatEvent::Done {
            text: "First reply".into(),
            title: "First".into(),
        });
        let second = view.start_send("second");
        view.apply_event_for_turn(first, ChatEvent::Learned {
            items: vec!["first memory".into()],
        });
        assert!(view.busy);
        let ThreadItem::Turn(first_turn) = &view.thread[1] else { panic!("first turn") };
        assert!(first_turn.notes.iter().any(|n| n.contains("first memory")));
        let ThreadItem::Turn(second_turn) = &view.thread[3] else { panic!("second turn") };
        assert!(second_turn.notes.is_empty());
        view.apply_event_for_turn(second, ChatEvent::Text { text: "Hi".into() });
        view.start_new();
        let third = view.start_send("third");
        assert_ne!(first, third);
        view.apply_event_for_turn(first, ChatEvent::Error { message: "stale".into() });
        assert!(view.busy, "a stale stream must not stop the current reply");
        let ThreadItem::Turn(turn) = view.thread.last().unwrap() else { panic!("third turn") };
        assert!(turn.failure.is_none());
    }

    #[test]
    fn event_flow_error_and_cancel() {
        let mut view = ChatView::new();
        view.start_send("hi");
        view.apply_event(ChatEvent::Error { message: "boom".into() });
        assert!(!view.busy);
        let ThreadItem::Turn(turn) = view.thread.last().unwrap() else {
            panic!("expected turn");
        };
        assert_eq!(turn.failure.as_deref(), Some("boom"));

        let mut view = ChatView::new();
        view.start_send("hi");
        view.apply_event(ChatEvent::Cancelled);
        assert!(!view.busy);
        let ThreadItem::Turn(turn) = view.thread.last().unwrap() else {
            panic!("expected turn");
        };
        assert!(turn.notes.iter().any(|n| n == "Stopped."));
    }

    #[test]
    fn approval_card_and_answer() {
        let mut view = ChatView::new();
        view.start_send("run it");
        view.apply_event(ChatEvent::Approval {
            id: "a1".into(),
            tool: "shell".into(),
            summary: "rm -rf /".into(),
            risk: "DANGEROUS".into(),
            detail: "rm -rf /".into(),
            always_rule: Some("shell: rm *".into()),
            can_always: true,
            can_session: true,
            tainted: false,
        });
        let ThreadItem::Turn(turn) = view.thread.last().unwrap() else {
            panic!("expected turn");
        };
        assert_eq!(turn.approvals.len(), 1);
        assert!(turn.approvals[0].danger);
        assert_eq!(turn.approvals[0].title, "Heron wants to run a command");
        let note = view.answer_approval("a1", ApprovalDecision::Once, None).unwrap();
        assert_eq!(note, "You allowed this.");
    }

    #[test]
    fn history_replay() {
        let mut view = ChatView::new();
        view.open_history(
            vec![
                HistoryMessage {
                    role: "user".into(),
                    content: Some("[system notice] hidden".into()),
                    tool_calls: vec![],
                    tool_call_id: None,
                },
                HistoryMessage {
                    role: "user".into(),
                    content: Some("list files".into()),
                    tool_calls: vec![],
                    tool_call_id: None,
                },
                HistoryMessage {
                    role: "assistant".into(),
                    content: Some("Here they are.".into()),
                    tool_calls: vec![HistoryToolCall {
                        id: "c1".into(),
                        name: "list_dir".into(),
                        arguments: r#"{"path": "/tmp"}"#.into(),
                    }],
                    tool_call_id: None,
                },
                HistoryMessage {
                    role: "tool".into(),
                    content: Some("a.txt\nb.txt".into()),
                    tool_calls: vec![],
                    tool_call_id: Some("c1".into()),
                },
            ],
            true,
        );
        // The system notice user message is skipped. One user bubble,
        // one turn, one interrupted note.
        assert_eq!(view.thread.len(), 3);
        assert!(matches!(view.thread[0], ThreadItem::User(_)));
        let ThreadItem::Turn(turn) = &view.thread[1] else {
            panic!("expected turn");
        };
        assert_eq!(turn.raw_text, "Here they are.");
        assert_eq!(turn.steps.len(), 1);
        assert_eq!(turn.steps[0].label, "Listed");
        assert_eq!(turn.steps[0].preview, "a.txt\nb.txt");
        assert!(matches!(view.thread[2], ThreadItem::Note(_)));
    }

    #[test]
    fn workspace_short() {
        let mut view = ChatView::new();
        view.workspace = "/Users/inan/work/projects/heron".into();
        assert_eq!(view.workspace_short(), "…/projects/heron");
        view.workspace = "/tmp".into();
        assert_eq!(view.workspace_short(), "/tmp");
        view.workspace = "".into();
        assert_eq!(view.workspace_short(), "your folder");
    }

    #[test]
    fn mode_parse_and_name() {
        assert_eq!(Mode::parse("readonly"), Some(Mode::ReadOnly));
        assert_eq!(Mode::parse("yolo"), Some(Mode::Yolo));
        assert_eq!(Mode::parse("nope"), None);
        assert_eq!(Mode::Yolo.name(), "yolo");
        assert_eq!(Mode::Ask.label(), "Ask first");
    }
}
