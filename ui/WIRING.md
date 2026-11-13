# Wiring: ChatView to MainWindow

Chat lane note for the Shell lane and the integration. The Shell bridge
contract is `~/workspace/HERON-Slint-Shell/docs/slint-bridge.md`. This
file says exactly how the Chat lane's view plugs into the shell's
window, and which Shell-side changes the wiring needs. The Chat lane
does not touch Shell files. The Shell lane applies the changes below.

## What the Chat lane wired on its side

- `ui/views/chat.slint` (`ChatView`) matches the window surface.
  Callback names match the window's where they correspond:
  `send-message`, `cancel-reply`, `new-session`, `approve`,
  `copy-requested`. The rest are ChatView callbacks the window must
  forward: `undo`, `set-mode`, `toggle-step`, `open-link`,
  `open-settings`, `continue-turn`, `use-starter`, `try-demo`,
  `resume`.
- `ui/components/message.slint` (`ChatMsg`) is a superset of the
  shell's `ChatMsg { role, text, body }`. It adds `steps`,
  `approvals`, `notes`, `failure`, `complete`, `live`. The shared
  fields have the same names and types. `text` is the plain source
  text. `body` is the rendered styled-text.
- `ui/components/theme.slint` mirrors the shell's Theme property for
  property (flat, dark values, full serif stack). The integration
  keeps one copy. Do not add properties to one copy without the other.
- `src/ui/chat.rs` (`ChatView` state machine, `ChatEvent`) is the
  verified spec for the event handling below. Its `ChatEvent` matches
  the shell's `bridge.rs` field for field except `Learned`: the shell
  carries `Vec<serde_json::Value>` and this file carries
  `Vec<String>`. The bridge stringifies each item when it converts.
  `always_rule` is `Option<String>` on both sides.
- Keyboard and the TextEdit composer live in this lane. Cmd/Ctrl+N
  starts a new conversation. Esc stops a running reply. Enter sends,
  Shift+Enter inserts a newline. The key handling follows the shell's
  FocusScope pattern. It is uncompiled, and the Send and Stop buttons
  already cover both actions if a key guess is wrong.
- Follow-scroll lives in this lane. The view tracks `follow` from the
  ScrollView position and shows the newest-message pill when the
  reader scrolled more than 240px from the bottom. It needs the
  `thread-version` counter below, because row mutations do not fire a
  model's changed handler.

## Embedding (main.slint, Shell lane)

Import `ChatView` from the chat lane and replace the chat branch's
`VerticalLayout` (the placeholder with the ListView and the LineEdit)
with it. Bind:

| ChatView property | Window source |
| --- | --- |
| `title` | `session-title` |
| `thread` | `messages` |
| `busy` | `busy` |
| `reduce-motion` | `reduce-motion` |
| `thread-version` | new window property, see below |
| `mode` | new window property `chat-mode` |
| `can-undo` | `session-id != ""` |
| `show-empty` | `messages` is empty (bind to the model length) |
| `has-key`, `demo`, `paused` | new window properties, see below |
| `workspace-short`, `workspace-full` | new window properties, see below |
| `starters` | new window property, see below |
| `hint` | keep the ChatView default (derived from `busy`) |

Bind the ChatView callbacks to the window callbacks of the same name
for `send-message`, `cancel-reply`, `new-session`, `approve`,
`copy-requested`. Forward the rest to the new window callbacks below.
Delete the placeholder ListView, the LineEdit composer, and the
Send/Stop/New buttons from the chat branch. Keep the session title
text out of the branch: the ChatView header owns it now.

## Window properties to add (Shell lane)

| Property | Type | Source |
| --- | --- | --- |
| `chat-mode` | string | `AppState.mode`, posted by the `ui_state` poll. The poll already fetches it. |
| `has-key` | bool | `AppState.has_key`, same poll. |
| `demo` | bool | `AppState.demo`, same poll. |
| `paused` | bool | `AppState.paused`, same poll. |
| `workspace-short` | string | Last path component of `AppState.workspace`. `workspace_short` in `src/ui/chat.rs` computes it. |
| `workspace-full` | string | `AppState.workspace`, same poll. |
| `starters` | `[string]` | `GET /suggestions` prompts (unanswered only). Fetch at startup and on `new-session`. The waiting and today blocks stay containers until the Views lane supplies their row shapes. |
| `thread-version` | int | Bumped by `models.rs` on every thread mutation (push, set_row, clear). The ChatView watches it with a `changed` handler to follow-scroll. |

## Window callbacks to add (Shell lane)

| Callback | Arguments | Meaning |
| --- | --- | --- |
| `set-mode(string)` | mode | `POST /api/mode`, then refresh state. |
| `undo()` | | `POST /api/undo` with the current session id. On `{undone: text}`, add the "Undone: {text}" note to the thread. |
| `toggle-step(string)` | step id | Flip `expanded` on the matching step row in the messages model. View state only, no server call. |
| `open-link(string)` | URL | Open it in the default browser with the `open` crate. |
| `use-starter(string)` | prompt text | Send it exactly like `send-message`. |
| `try-demo()` | | `POST /settings` with `{base_url: "demo"}`, then refresh state. |
| `resume()` | | `POST /pause` with `{paused: false}`, then refresh state. |
| `continue-turn()` | | Send "Continue from where you left off." in the current session, exactly like `send-message`. That is the web UI's continue behavior for interrupted turns. |
| `open-settings()` | | Set `current-view` to "settings". The Setup lane owns that view. |

## ChatMsg swap (models.rs, Shell lane)

Replace the shell's `ChatMsg { role, text, body }` with the chat
lane's richer struct. The generated Rust struct gains the new fields.
Update the constructors with these defaults:

- `push_message` (user row, note row): steps `[]`, approvals `[]`,
  notes `[]`, failure `""`, complete `true`, live `false`.
- The empty assistant row pushed by `on_send_message` before
  streaming: complete `false`, live `true`. It is the live turn.
- `set_row` in `append_reply_text` keeps the row's other fields and
  only updates `text` and `body`.

Tool messages never become rows. History replay folds them into the
turn's steps (see below). The window's model holds only
user, assistant, and note rows after the swap.

## apply_chat_event, richer behavior (models.rs, Shell lane)

The spec is `src/ui/chat.rs` (`ChatView::apply_event`, tested). The
current shell behavior is the placeholder; replace each arm:

- `Session { id }`: set `session-id`. Unchanged.
- `Text { text }`: append to the live turn's `text`, set `body` with
  `from_plain_text`. Never re-parse markdown here. Unchanged in
  shape; the row is the live turn.
- `ToolStart { id, name, summary, risk }`: add a step row to the live
  turn with state `"run"`. Label from `tool_label`, summary from
  `arg_summary`. Both live in `src/ui/chat.rs`.
- `ToolEnd { id, name, ok, preview }`: find the step by id, set state
  `"ok"` or `"err"`, set the preview (cut to 1500 chars), expand it
  when it is an error.
- `Approval { .. }`: attach a card to the live turn, do not push a
  placeholder message. Title from `ask_title`, warning from
  `approval_warning`, danger from `approval_danger`, labels from
  `session_button_label` and `always_button_label`. All live in
  `src/ui/chat.rs`. `always_rule` is `Option<String>`; None means no
  rule, and the always label falls back to the generic text.
- `Notice { text }`: append the text to the live turn's `notes`. Do
  not push a new row.
- `Model { name, why }`: append `"Using {name}: {why}."` to the live
  turn's `notes`.
- `Learned { items }`: attach the note to the **originating** reply
  even after Done, rather than checking only whether it is live.
  `ChatView::start_send` returns a monotonic id; each SSE forwarder
  must capture that id and use `apply_event_for_turn(id, event)`.
  This prevents late learning from attaching to a newer reply or from
  mutating a new conversation after the old one was cleared.
  Stringify each item first: the shell carries `Value`, the view
  carries `String`.
- `Usage { .. }`: ignore. Unchanged.
- `Done { text, title }`: set the live turn's `body` with
  `from_markdown` once, set `text` to the full reply, set
  `complete` true and `live` false, set `session-title`, set `busy`
  false.
- `Error { message }`: set the live turn's `failure` to the message,
  set `complete` true and `live` false, set `busy` false. Do not
  append the error to the reply text.
- `Cancelled`: close the live turn with a "Stopped." note, set
  `complete` true and `live` false, set `busy` false.
- Stream end with no terminal event: add a "connection closed" note
  to the live turn and close it. The forwarder task in `desktop.rs`
  knows the channel closed; it must track whether a terminal event
  arrived.
- Bump `thread-version` after every arm that mutates the model.

## History replay (Shell lane, when a session opens)

`get_session` returns `SessionDetail`. Replay it with the rules in
`src/ui/chat.rs` (`replay_history`): user messages become user rows,
assistant messages become turns, tool messages fold into the turn's
steps by `tool_call_id`, system-notice user messages are skipped.
`busy` and `interrupted` come from the payload. When `interrupted`
is true, show the "Continue from here" note; its button calls
`continue-turn()`.

## Gaps found while verifying (Shell lane)

- `stream_chat` errors never reach the UI. `desktop.rs` ignores the
  `Result` (`let _ =`). A 409 or a connection failure leaves the
  pushed empty assistant row live and `busy` true forever. Map the
  error into a `ChatEvent::Error` pushed to the channel before the
  sender drops, so the turn fails visibly.
- `copy-requested` has no Rust handler yet. The `arboard`
  dependency is declared (3.7.0). The ChatView fires it; the window
  forwards it; nothing handles it. Same for link activation (the
  `open` crate).
- `main.slint` imports `Theme` from `components/theme.slint`, but
  that file declares `global Theme` without `export`. If Slint
  requires the export, the first compile fails there. Worth one
  look before the first build.
- The `reduce-motion` window property exists, but nothing sets it
  from the OS setting yet. The ChatView reads it; the Shell lane
  wires the source.

## Open items (Chat lane)

- No `.slint` file has been compiler-verified in this sandbox. The
  first Mac or CI build is the check for the whole lane, including
  the new keyboard and scroll patterns.
- The `Key.Escape` spelling, the `event.modifiers` fields, and the
  FocusScope event ordering (whether the scope sees Return before
  the TextEdit consumes it) are the riskiest uncompiled guesses.
  Fallbacks: the Stop button covers Esc, the Send button covers
  Enter, and a window-level key handler in Rust is the fallback if
  the scope ordering is wrong.
- The `viewport-y` sign convention in the follow-scroll code is
  unverified: bottom is taken as `scroll.height - inner.height`.
  If the first render scrolls wrong, flip the sign there first.
- The Copy button's "Copied" label is optimistic. A clipboard
  failure still needs a bridge signal to show "Could not copy".
