# Chat view map: web frontend to Slint

Source of truth: `assets/index.html` (1074 lines) and `src/server.rs`.
Read on 2026-11-13 against main `ea06729a` (after PR #8).
This file is the Chat lane's map. It names what the Slint chat view must do,
what it sends, and what it receives. It does not guess the Shell bridge
contract. The bridge contract lands from the Shell lane. This file says what
the chat view needs from it.

## Plan alignment

The scoping plan at `~/workspace/HERON-Slint-Plan.md` governs this lane.
Slint is pinned `=1.18.1` with `slint-build =1.18.1`, default features,
winit backend, FemtoVG renderer. The plan's file layout puts this lane's
files at `ui/views/chat.slint`, `ui/components/message.slint`, and
`ui/components/approval_card.slint`. The plan's model types are `ChatMsg`
and `ToolStep`. The `.slint` structs here follow those names.

Markdown belongs to Slint. Replies render with
`slint::StyledText::from_markdown` (added in Slint 1.17). While a reply
streams, the view shows plain text. Markdown is rendered once, at `done`.
Nothing in this lane parses markdown. An earlier draft of `src/ui/chat.rs`
had its own markdown parser. It is deleted. It duplicated StyledText and
re-parsed the whole reply on every delta, which the plan names as the
sharpest risk.

Copy has no public Slint API. The `.slint` side fires a
`copy-requested(string)` callback and Rust copies with arboard. Links in
replies depend on the Phase 0 spike: if StyledText has no link-activated
callback, link clicks route through an `open-link(string)` callback to
Rust, which opens them with the `open` crate. Keyboard is this lane's
Phase 1 work: Cmd/Ctrl+N starts a new conversation, Esc stops a running
reply, Enter sends, Shift+Enter inserts a newline. It uses FocusScope
handlers that follow the shell's pattern in `ui/main.slint`. Menus are
the Shell lane's.

The Shell bridge contract has landed
(`~/workspace/HERON-Slint-Shell/docs/slint-bridge.md`) and this lane
is wired to it. The wiring, including the Shell-side changes the
integration needs, is in `ui/WIRING.md`. `ChatMsg` here is a superset
of the shell's `{ role, text, body }`. The Theme mirrors the shell's
flat dark Theme property for property. Callback names match the
window's where they correspond.

## Design direction

Inan's bar for the whole rebuild, Nov 13: "make it beautiful, more
interactive, more intuitive to use." Source for the craft bar:
`~/workspace/custom-skills/local-ui-excellence/SKILL.md` plus
`references/anti-slop.md` and `references/web.md`. What it means for
this lane, in the lane's own terms:

- Beautiful: the palette is Heron's own (slate `#2F6690`, talon
  `#C98F00`, serif headings), not a library default. The integrated
  Theme is the shell's flat dark Theme, mirrored property for
  property in `ui/components/theme.slint`. Light values stay
  documented in the Visual language section for a future light mode.
  The type scale is small and deliberate: 30px serif empty-state
  headline, 20px serif chat title, 16px semibold approval title,
  14px body and labels, 12px hints and tool detail. User bubbles are
  slate soft with a 12px radius and a 3px bottom-right corner. No
  gradients, no glass, no decorative numbering, no emoji standing in
  for icons.
- Interactive: state changes animate. Tool rows highlight on hover and
  press, and the state dot fades between run, ok, and err. The
  thinking indicator is three dots that pulse while a reply streams.
  Approval cards fade in. The composer border turns slate on focus.
  Every click gives visible feedback: the Send button is slate, Stop
  is red, a clicked Copy reads "Copied" for 1.6s. Motion budget
  matches the shell: 150ms for interactive feedback, 180ms for
  fades. Nothing runs longer than 300ms.
- Reduced motion: every animate block resolves to 0ms when the
  window's `reduce-motion` property is true. The ChatView takes it
  as a property and passes it down. The Shell lane sets the window
  property from the OS prefers-reduced-motion setting. Under reduced
  motion the thinking dots sit still and nothing pulses.
- Intuitive: the composer takes focus when the view appears. Send is
  disabled-looking while the input is empty. Stop is red and reads
  Stop while busy. Approvals read as one clear choice: Allow once is
  primary, the session and always options are secondary, Don't allow
  is destructive red, and the risk shows twice (card tint plus a left
  accent bar). Starter prompts are tappable cards, so the empty state
  invites the user in instead of showing a blank page.
- What stays native: standard chrome (Undo, mode buttons, banner
  actions) keeps the std-widgets Button and the pinned widget style
  (cupertino on macOS, fluent on Windows and Linux). The crafted
  button (`ui/components/action_button.slint`) is only for actions
  whose meaning is product-specific: Send, Stop, Allow, Deny, Copy,
  and the newest-message pill.

## Layout

The chat view is one column. A header bar holds the conversation title, the
Undo button, and the mode group. Below it sits the thread. Below the thread
sits the composer. The composer and the mode group are visible only on the
chat view. The rail with navigation and the recent conversation list is the
Views lane's. The chat view shows the title only. The rail shows the list.

## Header

- Title. Text is the session title, or "New conversation". The server sets
  the title. The `done` SSE event carries it. The client never invents one.
- Undo button. Hidden until a session is open. Tooltip: "Restore the last
  file Heron changed in this conversation". Clicking calls `POST /api/undo`
  with `{ session_id }`. See Undo below.
- Mode group. Four buttons, one pressed at a time. Values and labels:
  `readonly` "Read only", `ask` "Ask first", `auto` "Auto", `yolo`
  "Unrestricted". Pressed state comes from `GET /api/state` field `mode`.
  Clicking calls `POST /api/mode` with `{ mode }`, then refreshes state.
  The `yolo` click asks for confirmation first in the web client. The Slint
  view must show a confirm dialog instead. The confirm text is: "Unrestricted
  means Heron changes files and runs commands without asking. Continue?"
- Mode meanings, from the button titles and `src/permissions.rs`:
  `readonly` looks, searches and reads, never changes anything. `ask` asks
  before changing files or running commands. `auto` edits files and runs
  commands in the folder, and asks only before risky actions. `yolo` never
  asks. Only catastrophic commands are blocked.

## Thread

The thread is a vertical list of turns. One turn is one user message plus
Heron's reply. A user message renders as a right aligned bubble with a
slate tinted background. A reply renders as prose, a tool track, approval
cards, notes, and a failure block if the turn failed.

### Prose

Replies are markdown. The web renderer supports paragraphs, headings
(`#` to `####`, all rendered as one heading size), bulleted and ordered
lists, blockquotes, fenced code blocks, inline code, bold, italic, and
links (http and https only, opened in the default browser). The Slint
view renders replies with a StyledText element whose body the bridge
sets with `from_plain_text` while the reply streams and
`from_markdown` once the turn completes. While a reply streams, the
view shows plain text. Markdown is rendered once, when the turn
completes. Whether StyledText renders fenced code blocks, headings,
lists, and tables acceptably is the Phase 0 spike's open
question. Fallback, if it does not: the message delegate splits a reply
into segments, text segments as StyledText and code segments as
monospace Text in a bordered box. Do not discover this in Phase 1.

A finished reply gets a Copy button under the prose. Copy copies the raw
reply text, not the rendered form. The button shows "Copied" for 1.6
seconds, or "Could not copy" if the clipboard fails.

### Tool track

While Heron works, each tool call is one row in a vertical track with a
left rail. A row shows a label and a short argument summary. Clicking a
row expands its result text. An error result is always shown expanded.

- Label. A fixed map from tool name to a past tense verb. `shell` is "Ran",
  `read_file` is "Read", `write_file` is "Wrote", `edit_file` is "Edited",
  `list_dir` is "Listed", `search_files` is "Searched files for",
  `find_files` is "Looked for files", `web_fetch` is "Opened",
  `browser_open` is "Opened in the browser", `browser_read` is "Looked at
  the page", `browser_click` is "In the browser:", `browser_type` is "In
  the browser:", `web_search` is "Searched the web for", `remember` is
  "Remembered", `recall` is "Checked memory for", `forget` is "Forgot
  memory", `task_add` is "Added task", `task_list` is "Checked tasks",
  `task_update` is "Updated task", `schedule_add` is "Scheduled",
  `schedule_list` is "Checked reminders", `schedule_remove` is "Cancelled
  reminder", `use_skill` is "Loaded skill", `delegate` is "Handed off",
  `save_automation` is "Set up automation", `update_persona` is "Changed
  personality", `save_skill` is "Saved skill", `search_knowledge` is
  "Searched your email, calendar and documents for", `if` is "Checked a
  condition", `notify` is "Told you", `ask` is "Asked the AI model",
  `agent` is "Worked on it", `for_each` is "Repeated for each item", `set`
  is "Worked out a value", `stop` is "Stopped". Unknown names strip a
  leading `mcp__` and turn `__` into ` → `.
- Argument summary. The tool arguments arrive as a JSON string. The
  summary takes the first present field in this order: `command`, `path`,
  `url`, `query`, `pattern`, `glob`, `task`, `title`, `content`, `name`.
  A schedule shape uses `when` and `prompt` as "{when} — {prompt}". A bare
  `id` renders as "#{id}". The summary is cut to 160 characters.
- Row states. Running, ok, error. Running shows a pulsing amber dot. Ok
  shows a filled slate dot. Error shows a red dot and the result in red.
- Result text. Cut to 1500 characters. A result starting with `Error:` or
  `Permission denied` marks the row as an error.

### Approval cards

An approval request arrives mid stream as an SSE `approval` event. It
renders inline in the thread, inside the current turn. It is a bordered
card with a title, a warning line when one applies, a detail block, and
answer buttons. Answering disables all buttons. The card is then replaced
by a one line note stating what was decided.

- Title. A fixed map from tool name. `shell` is "Heron wants to run a
  command", `write_file` is "Heron wants to write a file", `edit_file` is
  "Heron wants to edit a file", `read_file` is "Heron wants to read a
  sensitive file", `list_dir` is "Heron wants to open a sensitive folder",
  `search_files` is "Heron wants to search a sensitive folder",
  `save_automation` is "Heron wants to set up an automation",
  `update_persona` is "Heron wants to change its personality", `save_skill`
  is "Heron wants to save a skill for next time". Any other tool is
  "Heron wants to use {name}", with `mcp__` stripped and `__` turned into
  ` → `.
- Warning lines, in order. Standing change tools (`save_automation`,
  `update_persona`, `save_skill`): "This changes what Heron does from now
  on, so it always needs your OK. Read it through before allowing."
  Risk `DANGEROUS`: "This could delete data, reach outside your folder,
  or be hard to undo." Risk `sensitive`: "This reads something that may
  hold passwords, keys or other secrets, and would show it to your AI
  model." Risk `external`: "This acts on a website or another app. Heron
  cannot undo it." Risk `egress`: "This sends text Heron wrote to
  somewhere outside your computer. Check that nothing private is in it."
  Tainted (Heron has read outside content in this conversation): "Heron
  has read content from outside (a web page, an email or a connected app)
  in this conversation. Check that this is something you asked for."
- Danger styling. The card turns red when the risk is `DANGEROUS` or
  `sensitive`, except for the three standing change tools, which stay amber.
- Detail block. The `detail` field, or the `summary` field when detail is
  empty. Monospace, scrollable, 220px max height.
- Buttons. "Allow once" is always present. "Allow for this conversation"
  appears when `can_session` or `can_always` is true. Its label is "Allow
  searches for this conversation" for `web_search`. "Always allow this
  kind of action" appears when `can_always` is true. Its label for `shell`
  is `Always allow "{rule}" commands`, where rule is `always_rule` with
  the `shell:` prefix and a trailing ` *` stripped. "Don't allow" is
  always present. The first button takes keyboard focus when the card
  appears.
- Decisions. `once`, `session`, `always`, `deny`. Sent as
  `POST /api/approve` with `{ id, decision }`. The note after answering
  reads "You allowed this." for once, "You allowed this, and the same
  command for the rest of this conversation." for a shell session grant,
  "You allowed this, and the same kind of action for the rest of this
  conversation." for other session grants, "You allowed this, and similar
  actions from now on." for always, and "You didn't allow this." for deny.
  If the approve call fails, the note shows the error message instead.
- Timeout. If nobody answers within 900 seconds, the server declines with
  "no answer to the approval request". The client shows nothing extra. The
  turn then continues or fails on the server side.

### Notes and failures

- Note lines. Small muted text inside a turn. Used for model switches
  ("Using {name}: {why}."), memory saves ("Saved to memory: {items
  joined with ' / '}"), notices from the server, and "Stopped." after a
  cancel.
- Failure block. Red bordered box. Starts with "Heron could not finish."
  followed by the message. When the message mentions a key or a 401 or
  403, it adds an "Open settings" button.
- Connection closed. When the stream ends without a terminal event, the
  turn gets the note "The connection to Heron closed before the reply
  finished."

### Thinking indicator

While the reply has no text yet, and between tool steps, the turn shows
three pulsing dots and a label. Labels: "Thinking…" at the start and after
a `notice`, "Working out the next step…" after a `tool_end`, "Thinking…"
after a `session` event. The indicator is removed when the first text
delta arrives.

### Scroll follow

The thread follows the reply as it grows, unless the reader has scrolled
up. "Scrolled up" means more than 240px from the bottom. The decision is
taken when the reader scrolls, not when content lands. When the reader is
not following and a reply is in progress, a "↓ Newest message" pill
appears. Clicking it jumps to the bottom and resumes following.
Mechanism: the view tracks `follow` from the ScrollView position, and
Rust bumps a `thread-version` counter on every thread mutation. The
view watches the counter with a `changed` handler and snaps to the
bottom while following. Row mutations do not fire a model's changed
handler, so the counter is the trigger. The bottom position is taken
as `scroll.height - inner.height`; the sign convention is unverified.

## Empty state

A new conversation with no messages shows the empty state inside the
thread. Its parts, in order:

1. Connect card, only when no model is configured. Amber border. Text:
   "Heron needs an AI model before it can reply. Add an API key, or point
   it at a model running on this computer." Buttons: "Connect a model"
   (opens settings) and "Try the demo first" (sets base_url to `demo`,
   refreshes state, shows the empty state again).
2. Demo card, only in demo mode. "Demo mode: replies are canned, but the
   actions are real, so you can try approvals, memory and reminders.
   Connect a model for real answers." Button: "Connect a model".
3. Paused card, only when paused. "Heron is paused: it starts nothing on
   its own until you resume it. You can still ask it for things here."
   Button: "Resume" (posts `{ paused: false }` to `/api/pause`).
4. Waiting approvals block. Data from `GET /api/approvals`. Owned by the
   Views lane. The chat view renders the container.
5. Suggestions block. Data from `GET /api/suggestions`. Owned by the
   Views lane. The chat view renders the container.
6. Today block. Data from `GET /api/today`. Owned by the Views lane. The
   chat view renders the container.
7. Heading "What should we get done?" and a line naming the workspace
   folder. The folder path shortens to the last two segments with a "…/"
   prefix when it is longer. The full path is the hover text.
8. Four starter prompts as buttons. Clicking one fills the composer and
   focuses it. The prompts are: "Look through this folder and tell me
   what is in it", "Remind me tomorrow at 9:00 to call the dentist",
   "Research the best way to learn Rust in a month and save a plan as
   plan.md", "What do you remember about me?"

## Composer

- A text area and a Send button. The hint under it reads "Enter to send,
  Shift+Enter for a new line". The text area grows with content up to 38
  percent of the window height.
- Enter sends. Shift+Enter inserts a newline. While a reply is running,
  the button reads "Stop" and is red. Clicking it stops the run. Escape
  also stops the run. While running, the hint reads "Heron is working.
  Press Esc or Stop to halt it."
- Sending is disabled while a reply is running in the same conversation.
  The server also rejects a second concurrent run with HTTP 409 and the
  message "Heron is still replying in this conversation. Stop it first,
  or wait." The client shows that as a failure block.
- Cmd or Ctrl+N starts a new conversation. That shortcut is the Shell
  lane's. The chat view exposes a `new-conversation` action for it.

## Opening a session

`GET /api/sessions/{id}` returns `{ id, title, messages, busy,
interrupted }`. The client sets the title, renders the history, shows
Undo, and refreshes the session list. If `interrupted` is true, the thread
gets a note "This reply was cut short before Heron finished." and a
"Continue from here" button. Clicking it removes the note and sends
"Continue from where you left off."

History replay rules, from `renderHistory`:

- A user message renders as a bubble. A user message starting with
  `[system notice]` is skipped.
- An assistant message with content appends the text to the current turn.
  Its tool calls each add a finished (ok) step row. When an assistant
  message has content and no tool calls, the turn is closed.
- A tool message closes the step with the matching `tool_call_id`. The
  result text is cut to 1500 characters. A result starting with `Error:`
  or `Permission denied` marks the step as an error.
- An empty history shows the empty state.

## Undo

`POST /api/undo` with `{ session_id }` returns one of four shapes:
`{ undone: null }` means nothing to undo. `{ undone: "text" }` means
restored, and the note reads "Undone: {text}". `{ undone: null, stale:
"reason" }` means the note reads the stale reason as given.
`{ undone: null, conflict: "reason", checkpoint: "id" }` means the file
changed since Heron changed it. The client shows a dialog titled "Undo
would overwrite newer changes", with the conflict reason, the line "If
you restore anyway, the version that is there now is saved beside the
file first, so nothing is lost.", and two buttons: "Leave the file as it
is" and "Restore anyway". Restore anyway calls `POST /api/undo` again
with `{ session_id, force: true, checkpoint: "id" }`. The session id and
checkpoint in that second call are the ones from the conflict answer, so
a conversation switch in between cannot redirect the restore. A failed
call shows "Could not undo: {error}".

## API calls the chat view makes

Base path is `/api`. Every call sends the `x-heron-token` header with the
per launch token. The web client reads the token once from the launch URL
query string, keeps it in session storage, and strips it from the address
bar. The bridge owns the token. The chat view never sees it. All bodies are JSON.

| Call | Purpose | Request | Response |
|---|---|---|---|
| `GET /state` | Mode, model, demo, paused, workspace, token counts | none | `{ name, version, model, mode, workspace, has_key, demo, memories, unread, paused, waiting, onboarded, suggestions, mcp, usage: { prompt_tokens, completion_tokens }, tools, os, sandbox }` |
| `GET /sessions` | Recent conversation list (rail data, Views lane) | none | `[{ id, title, created_at, updated_at, messages }]` newest first, only sessions with messages shown |
| `GET /sessions/{id}` | Open a conversation | none | `{ id, title, messages: [{ role, content, tool_calls: [{ id, type, function: { name, arguments } }], tool_call_id }], busy, interrupted }` |
| `POST /sessions` | Create an empty session (unused by the web client today) | none | `{ id }` |
| `DELETE /sessions/{id}` | Delete a conversation (rail action, Views lane) | none | `{ ok: true }` |
| `POST /chat` | Send a message, SSE stream back | `{ session_id: string \| null, message: string }` | SSE, see below |
| `POST /approve` | Answer an approval card | `{ id, decision: "once" \| "session" \| "always" \| "deny" }` | `{ ok: true }` |
| `POST /cancel` | Stop a running reply | `{ session_id }` | `{ ok: true }` |
| `POST /undo` | Restore the last file change | `{ session_id }` or `{ session_id, force: true, checkpoint }` | See Undo |
| `POST /mode` | Change permission mode | `{ mode: "readonly" \| "ask" \| "auto" \| "yolo" }` | `{ mode }` |
| `POST /pause` | Pause or resume background work | `{ paused: bool }` | `{ paused: bool }` |
| `POST /settings` | Demo mode switch from the empty state | `{ base_url: "demo" }` | settings shape |

Calls the empty state embeds but the Views lane owns: `GET /approvals`,
`POST /approvals/{id}` with `{ action: "approve" | "dismiss" }`,
`GET /suggestions`, `POST /suggestions/{id}` with `{ action }`,
`GET /today`.

## SSE event shapes (POST /chat)

Events arrive as `data: {json}\n\n`. The server sends a keep alive every
15 seconds. The first event is always `session`. Terminal events are
`done`, `error`, and `cancelled`. Everything else is a mid stream update.

| `type` | Fields | Chat view behaviour |
|---|---|---|
| `session` | `id` | Sets the current session id. Shows Undo. |
| `text` | `text` | Appends to the live prose block. Removes the thinking indicator. |
| `tool_start` | `id`, `name`, `summary`, `risk` | Adds a running step row. `summary` is the argument summary. `risk` is not displayed on the row. |
| `tool_end` | `id`, `name`, `ok`, `preview` | Closes the step. `preview` is the result text. |
| `approval` | `id`, `tool`, `summary`, `risk`, `detail`, `always_rule`, `can_always`, `can_session`, `tainted` | Shows an approval card. See Approval cards. |
| `notice` | `text` | Adds a note line. |
| `model` | `name`, `why` | Adds a note "Using {name}: {why}." |
| `learned` | `items: [string]` | Adds a note "Saved to memory: {items joined with ' / '}". |
| `usage` | `prompt`, `completion` | No thread display. The footer token count comes from a state refresh. The chat view ignores this event. |
| `error` | `message` | Closes the turn. Shows a failure block. Ends the run. |
| `cancelled` | none | Closes the turn. Adds a "Stopped." note. Ends the run. |
| `done` | `text`, `title` | Closes the turn. Sets the conversation title. Refreshes the session list and state. Ends the run. |

Error handling on the request itself: a non 2xx response carries
`{ error }` in JSON. The client throws that message into a failure block.
An empty message is rejected client side before sending.

## Data interface: what the chat view needs from the bridge

The bridge is the Shell lane's. It owns HTTP, SSE parsing, the token, and
the event loop. The chat view owns the thread model and the components.
The interface below is what the chat view needs. It is written against
the server shapes above, not against any bridge draft. If the Shell
bridge publishes a different shape, this section is the checklist to
reconcile against.

### Outbound: the view calls these bridge operations

- `send_message(session_id: string | null, text: string)`. Opens the SSE
  stream. Delivers events to the view one at a time, in order. Reports a
  request failure (non 2xx, or a network error) as one error value with
  the server's `{ error }` text when present.
- `answer_approval(id: string, decision: string)`. Returns ok or an error
  message. The view shows the error message as the decision note.
- `cancel_run(session_id: string)`. Best effort. The view does not wait
  for it before showing "Stopped." The `cancelled` SSE event is the
  authoritative end.
- `undo(session_id: string, force: bool, checkpoint: string | null)`.
  Returns the undo response shape above.
- `set_mode(mode: string)`. Returns the new mode name.
- `set_paused(paused: bool)`. Returns the new state.
- `fetch_state()`. Returns the state shape above.
- `fetch_session(id: string)`. Returns the session shape above.
- `set_demo()`. Posts `{ base_url: "demo" }` to settings. Used only by
  the empty state.

### Inbound: the bridge delivers these to the view

- One `ChatEvent` per SSE event, in arrival order, with the fields in the
  table above. The view applies them to its thread model. See
  `src/ui/chat.rs` for the `ChatEvent` type and the state machine. Text
  deltas only append to the raw text. The `.slint` side shows plain text
  while the turn is live and renders markdown once it completes.
- A stream end with no terminal event. The view adds the "connection
  closed" note. The bridge must distinguish a clean end from a network
  break if it can. Either way the view shows the same note.
- A request failure before the first event. Delivered as one error value.
  The view shows it as a failure block.

### State the view keeps

- Current session id, or none for a new conversation.
- Conversation title.
- Permission mode.
- Busy flag. True from send until a terminal event, an error, or a
  cancel.
- Thread items. User bubbles, turns, and notes, in order. The Rust side
  keeps `ThreadItem`. The binding maps each item to a `ChatMsg` row for
  the `.slint` side: `User(text)` becomes role "user" with body
  `from_plain_text(text)` and text the text, `Turn` becomes role
  "assistant" with body `from_plain_text(text)` while streaming and
  `from_markdown(text)` at done, text the raw reply text for the Copy
  button, plus its steps, approvals, notes, failure, complete, and
  live flags, `Note(text)` becomes role "note" with body
  `from_plain_text(text)` and text the text.
- Follow flag. True unless the reader scrolled more than 240px from the
  bottom. The view, not the bridge, owns this.
- Expanded step ids. Which tool result rows are open. Error rows start
  open. Other rows start closed.
- Answered approval ids. A card that was answered is replaced by its
  decision note and never reappears.

### What the chat view does not own

- The token, the HTTP client, SSE parsing, reconnect policy. Shell lane.
- The rail, the session list, the navigation. Views lane.
- The settings, memory, reminders, awareness, usage, and activity pages.
  Setup and Views lanes.
- Window chrome, menus, keyboard shortcuts, clipboard access, opening
  links in the default browser. Shell lane. The chat view calls out
  through named callbacks for copy, open link, and open settings.
- The Slint version pin, the build setup, and the generated component
  bindings. Shell lane.

## Visual language (from the web CSS)

Light theme: paper `#EDF1F4`, surface `#F9FBFC`, ink `#16212B`, muted
`#566878`, line `#D2DAE1`, slate `#2F6690`, slate soft `#DCE7EF`, talon
`#C98F00`, talon soft `#FBF0CC`, talon ink `#4A3600`, danger `#B3362B`,
danger soft `#F8E1DE`, ok `#2B7A57`. Dark theme: paper `#10181F`,
surface `#17212A`, ink `#E3EAF0`, muted `#93A4B3`, line `#293642`,
slate `#7DB3D8`, slate soft `#1D3040`, talon `#EDB92B`, talon soft
`#352C0F`, talon ink `#F6E3A6`, danger `#EC7A6F`, danger soft `#3A1D1A`,
ok `#6EC49C`. The integrated Theme is the shell's flat dark Theme,
mirrored property for property in `ui/components/theme.slint`. Light
values stay here for a future light mode. Serif for headings and the
empty state headline. Sans for body. Monospace for tool arguments,
results, and approval details.
User bubbles are slate soft with a 12px radius and a 3px bottom right
corner. Tool steps are 14px muted text on a left rail. Approval cards
are amber, red when dangerous. The composer is a bordered text area with
a slate Send button that turns red and reads Stop while busy.

## Open items

- The Shell bridge contract has landed and this lane is wired to it.
  The wiring, including the Shell-side changes the integration needs,
  is in `ui/WIRING.md`. What is still open is all on the Shell side:
  the ChatMsg swap in `models.rs`, the richer `apply_chat_event`,
  the new window properties and callbacks, and the `stream_chat`
  error gap (a failed stream leaves a live row and `busy` true).
- The Slint version is pinned (`=1.18.1`), but no Slint toolchain is
  installed in this sandbox. The `.slint` drafts in this lane are not
  compiler verified. The toolchain itself is now verified: PR #9's
  "Tests" step passed on all three OSes in CI (Nov 13), so crates.io
  is reachable there and the Slint compiler runs. What is still
  unverified is this lane's `.slint` files compiling. The Shell lane
  owns the first compile.
- Whether StyledText renders fenced code blocks, headings, lists, and
  tables acceptably is a Phase 0 spike question. The fallback (message
  delegate splits the reply into text and code segments) is named in the
  Prose section. The `@markdown()` spelling question is gone: the
  `.slint` side no longer uses the macro. The bridge sets the styled
  body with `from_plain_text` and `from_markdown` in Rust, which the
  spike already exercised.
- Whether StyledText exposes a link-activated callback is a spike
  question. If it does not, link clicks route through the `open-link`
  callback to Rust.
- Keyboard is wired in this lane now (FocusScope handlers in
  `ChatView` and `ComposerView`), but it is uncompiled. The riskiest
  guesses: the `Key.Escape` spelling, the `event.modifiers` fields,
  and whether the FocusScope sees Return before the TextEdit consumes
  it. Fallbacks: the Stop button covers Esc, the Send button covers
  Enter, and a window-level key handler in Rust is the fallback if
  the scope ordering is wrong.
- The follow-scroll `viewport-y` sign convention is unverified:
  bottom is taken as `scroll.height - inner.height`. If the first
  render scrolls wrong, flip the sign there first.
- Style decisions, all three settled Nov 13: (1) the "native look" bar,
  "Native window, coherent look, no webview. Slint clears it"; (2) the
  widget style, by Inan's delegation ("just make it good so its in your
  hands"): Cookie's call is cupertino on macOS, fluent on Windows and
  Linux, pinned per target by the Shell lane in build.rs; (3) the license
  route, royalty-free Slint with AboutSlint attribution in the About
  dialog. This lane builds on that call: standard Slint widgets only, no
  style pinned in any lane file, and the palette (slate #2F6690, talon
  #C98F00, serif headings via Theme.heading-font) carried as custom
  theme properties.
- The empty state waiting, suggestions, and today blocks need row shapes
  from the Views lane. The chat view renders containers only until then.
- The `usage` SSE event is ignored by the web client. If the Shell lane
  wants live token counts in the footer, that is a Shell concern, not a
  chat view concern.
- The web client focuses the first approval button when a card appears.
  Focus behaviour in Slint is a Shell concern. The chat view exposes
  which card is newest.
- The design-direction work adds Slint patterns that need compile
  verification in the Phase 0 spike: `Timer` elements, the `init`
  callback, `animate` blocks whose duration is a property expression,
  `has-focus` on TextEdit, `has-hover` and `pressed` on TouchArea, and
  per-corner radius on Rectangle. All are uncompiled, same as the rest
  of the `.slint` drafts.
- CSS-shorthand lesson from PR #9 (Nov 13): the Slint parser rejects
  multi-value `padding` (`padding: 18px 12px 12px;` failed the build
  on all three OSes). Slint's `padding` takes a single length; the
  fix is the individual sides. Checked this lane Nov 13: zero
  occurrences. No `padding:` or `margin:` property is used anywhere
  in the lane's `.slint` files, no `border:` or `font:` shorthand,
  and no multi-value `spacing` or `border-radius`. Spacing comes
  from single-value `spacing` on the layout boxes and single-value
  `border-radius` plus per-corner radius properties.
- The Shell lane sets the window's `reduce-motion` property from the OS
  prefers-reduced-motion setting. The chat view only reads it.
- The Copy button's "Copied" label is optimistic. A clipboard failure
  still needs a bridge signal to show "Could not copy". That signal is
  not in the bridge contract yet.
