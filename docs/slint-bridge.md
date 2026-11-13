# Slint bridge contract

Shell lane note for the Chat, Views, and Setup lanes. This is the interface you
code against. The backend and the server API are unchanged.

## Pinned toolchain

- `slint` **1.18.1**, latest stable on crates.io (verified against the crates.io
  API and the sparse index, 2026-11-13). `slint-build` is pinned to the same
  version, in lockstep. Default features: `std`, `backend-default` (winit on
  desktop), `renderer-femtovg`, `renderer-software`, `accessibility`,
  `compat-1-2`, `system-tray`. The `compat-1-18` feature is enabled: the plan
  calls it mandatory for the 1.18 line. The crate needs Rust 1.92 or newer; the
  sandbox has 1.99.0. Do not add `renderer-skia` or `backend-qt` in Phase 0.
- The `desktop` cargo feature now means the Slint window. It used to mean the
  WebView (wry/tao/muda), which is removed. Every existing invocation keeps
  working: `cargo build --features desktop --bins`, CI, and the packaging
  scripts all build the Slint window through this flag.
- `arboard` **3.7.0** and `rfd` **0.18.0** are declared as optional deps behind
  the `desktop` feature (latest stable, verified against the crates.io API,
  2026-11-13). Both are wired now: the `copy-requested` handler in `main.rs`
  copies through arboard, and the documents folder picker in
  `setup_glue.rs` opens `rfd::AsyncFileDialog` inside `slint::spawn_local`.
- **The build is NOT verified in this sandbox.** `cargo build --features desktop
  --bins` failed during dependency resolution: crates.io egress through the
  proxy is degraded (index fetches stall, static downloads run at about 1 KB/s).
  No slint crate is in the local registry cache, so the Slint tree cannot be
  fetched here in reasonable time. Verification must run on Inan's Mac or in
  CI. Never claim the toolchain is verified from this sandbox. The lib still
  compiles without the feature; check that with plain `cargo check`.

## Layout (bin crate, behind the `desktop` feature)

All Slint code lives in the desktop binary crate, never the lib:

- `src/bin/desktop/main.rs` — entry point. Starts the backend, creates the window,
  installs the message model, wires the Chat, Views, and Setup callbacks, runs
  the event loop.
  It is `main.rs` inside `src/bin/desktop/`, not `src/bin/desktop.rs`: for a
  file crate root, a bare `mod bridge;` resolves to a sibling
  (`src/bin/bridge.rs`), never to `src/bin/desktop/bridge.rs`. The directory
  form is what makes the modules below resolve. The plan's layout had
  this wrong (file root plus `desktop/` modules), and the compiler failed
  all four with E0583. Moving the four modules out to `src/bin/` instead
  was tested and rejected: Cargo auto-discovers every `src/bin/*.rs` file
  as a binary, so each module would fail with E0601 (`main` function not
  found). Do not use `#[path]` to work around either rule.
- `src/bin/desktop/bridge.rs` — `HeronClient`, `ChatEvent`, `AppState`,
  `SessionDetail`, `Bridge`. This is the API the other lanes call.
- `src/bin/desktop/sse.rs` — SSE line parsing, split out so it is testable
  without a server or a window.
- `src/bin/desktop/models.rs` — the window's message model and
  `apply_chat_event`.
- `src/bin/desktop/ui_state.rs` — the live server-state poll (every 30s).
- `src/bin/desktop/views_bridge.rs` — `HeronViewsBridge`, the real
  `ViewsBridge` implementation for the Views lane.
- `src/bin/desktop/views_slint.rs` — `ViewsController` and `bind_window`,
  the glue that drives the sidebar and the six views.
- `src/bin/desktop/setup.rs` — the Setup lane's pure view state plus the
  JSON mapping (the `SetupBridge` trait, `OnboardingState`,
  `SettingsState`).
- `src/bin/desktop/setup_bridge.rs` — `HeronSetupBridge`, the real
  `SetupBridge` implementation.
- `src/bin/desktop/setup_glue.rs` — `SetupDriver` and `bind_window`, the
  glue that drives onboarding and settings.
- `ui/main.slint` — the shell window (`MainWindow`), per the plan. The file is
  named `main.slint`, not `heron.slint`: a generated `mod heron` would shadow
  the `heron` lib crate in the binary, so the plan's name is the only one that
  works. It holds the MenuBar, the sidebar, the view switcher, and all nine
  views.
- `ui/components/theme.slint` — the `Theme` global (dark palette, serif
  headings). A copy of the Setup lane's file with one word added: `export`,
  which Slint requires for a global another file imports. The integration
  keeps one copy, and it must be the exported one. Do not edit the values in
  one worktree without the other.
- `build.rs` compiles only the root file `ui/main.slint` with
  `slint_build::compile_with_config`, pinning the widget style per target
  (settled 2026-11-13): cupertino on macOS, fluent on Windows and Linux. The
  dark palette (slate #2F6690, talon #C98F00, serif headings) rides as custom
  theme properties in the .slint files. The style-pinning call is NOT verified
  in this sandbox; the first Mac or CI build is the check for it. One root
  only, on purpose: each `compile_with_config` call sets the
  SLINT_INCLUDE_GENERATED variable, last call wins, and `include_modules!()`
  reads exactly that one path, so compiling a second top-level file would
  silently drop MainWindow from the generated code. Lanes add their UI as
  modules imported from main.slint, never as separate compile roots. Every
  .slint file under ui/ is still watched for rebuilds.

The Shell lane owns `ui/main.slint` (window, properties, callbacks) and
`src/bin/desktop*`. The Chat lane owns the chat view layout inside the window
and the richer chat rendering. The Views lane owns the sidebar, sessions,
memory, reminders, and awareness panels. The Setup lane owns onboarding,
providers/settings, and persona. The About dialog is its own exported
`Dialog` component in `ui/main.slint`, shown from Rust as a separate native
window (see the license note).

## Window surface (`MainWindow` in `ui/main.slint`)

Properties the bridge drives (views read them, never set them directly except
through the bridge):

| Property | Type | Meaning |
| --- | --- | --- |
| `backend-status` | string | One-line server state, e.g. `Heron · model · mode`. Polled every 30s. |
| `error-message` | string | Startup failure text. Empty when the backend started. While non-empty the ChatView is hidden and the error shows instead, so a failed start never eats the user's text. |
| `session-id` | string | Current conversation id, owned by Rust. Empty means "no session yet"; the server creates one lazily on the first message sent. |
| `session-title` | string | Title of the current conversation. |
| `messages` | `[ChatMsg]` | Current conversation, in order. `ChatMsg` is `{role, text, body, steps, approvals, notes, failure, complete, live}` where `role` is `user`, `assistant`, or `tool`, `text` is the plain source text, and `body` is the rendered `styled-text`. |
| `busy` | bool | A reply is streaming in the current conversation. |
| `chat-mode` | string | Server mode (`readonly`, `ask`, `auto`, `yolo`), kept in sync by the state poll. |
| `has-key` | bool | An API key (or a local/demo model) is configured. |
| `demo` | bool | Demo mode is on. |
| `paused` | bool | Heron is paused. |
| `workspace-short` | string | Short workspace path for the chat header. |
| `workspace-full` | string | Full workspace path. |
| `starters` | `[string]` | Suggestion prompts for the empty state. |
| `thread-version` | int | Bumps on every thread mutation so the view can follow the scroll to the bottom. |
| `current-view` | string | Active view id (`chat`, `onboarding`, `awareness`, `memory`, `tasks`, `reminders`, `usage`, `activity`, `settings`). Set by the Sidebar navigation, the Shell, and the Setup driver (onboarding at startup). |
| `reduce-motion` | bool | Animations run at 0ms while this is on. Default off. Wiring to the OS setting is still open. |

The Views lane's forwarded properties and callbacks (`sidebar-*`,
`memory-*`, `tasks-*`, `reminders-*`, `awareness-*`, `activity-*`,
`usage-*`) live in `docs/slint-views-bridge.md`. The Setup lane's
(`onboarding-*`, `settings-*`) are declared in `ui/main.slint` and
driven by `src/bin/desktop/setup_glue.rs`; the per-view contract is in
`ui/setup-bridge.md` and `ui/setup-map.md`.

Callbacks the bridge handles:

| Callback | Arguments | Meaning |
| --- | --- | --- |
| `send-message(string)` | typed text | Send a message in the current conversation. |
| `new-session()` | | Stop the running reply on the server, forget the server session id, and clear the view. The server creates the next session lazily on the first message. Also invalidates the old turn, so its forwarder drops any event still in flight. |
| `cancel-reply()` | | Stop the running reply (`POST /api/cancel`). |
| `approve(string, string)` | approval id, decision | Answer a waiting approval. Decision is `once`, `always`, `session`, or anything else to deny. |
| `set-mode(string)` | mode | Switch the server mode (`readonly`, `ask`, `auto`, `yolo`) through `POST /api/mode`. |
| `undo()` | | Undo the last change in the current conversation (`POST /api/undo`). A conflict surfaces as a note; force restore has no UI yet. |
| `toggle-step(string)` | step id | Expand or collapse a tool step in the thread. |
| `open-link(string)` | URL | Open a link from a message through the `open` crate. |
| `use-starter(string)` | prompt | Send one of the empty-state starter prompts. |
| `try-demo()` | | Switch to demo mode (`POST /api/settings` with `base_url: demo`). |
| `resume()` | | Unpause Heron (`POST /api/pause` with `paused: false`). |
| `continue-turn()` | | Send "Continue from where you left off." in the current conversation. |
| `open-settings()` | | Switch to the settings view. |
| `show-about()` | | Open the About dialog (a separate native window, shown from Rust). |
| `copy-requested(string)` | text to copy | Programmatic copy. There is no Slint copy API, so Rust copies through the `arboard` crate. |

The window has a real macOS menu bar via Slint's `MenuBar`: File has "New
conversation", Edit has "Copy" (`role: MenuItemRole.copy`). MenuItem roles
move About, Hide, and Quit into the application menu. Only the two items above
are spike-verified; do not guess other `MenuItemRole` variant names.

## Rust API (`src/bin/desktop/bridge.rs`, desktop feature only)

- `HeronClient::from_launch_url(url)` splits the backend launch URL
  (`http://127.0.0.1:PORT/?token=...`) into origin and token.
  `HeronClient::new(origin, token)` builds one directly. Clone is cheap.
- `client.api(method, path, body)` is the generic JSON call for every endpoint
  below: `GET /api/state`, `POST /api/mode` with a body, and so on. The path
  starts with `/api`. A non-2xx response becomes an error carrying the server's
  `{"error": "..."}` message.
- Typed helpers: `state()`, `list_sessions()` (returns `SessionListRow`, the
  bridge's own shape mirroring the server's `SessionInfo`, which only derives
  `Serialize`), `new_session()`, `get_session(id)`,
  `delete_session(id)`, `approve(id, decision)`, `cancel(session_id)`,
  `set_mode(mode)`, `undo(session_id)` (returns `UndoOutcome`: undone,
  nothing, stale, or conflict), `undo_force(session_id, checkpoint)`,
  `try_demo()`, `resume()`, `starters()` (live suggestions, falling back
  to the four built-in prompts).
- `client.stream_chat(session_id, message, tx)` posts to `/api/chat` and pushes
  `ChatEvent` values into the channel in stream order. The stream ends when the
  server closes it (after `done`, `error`, or `cancelled`). Keep-alive comment
  lines are skipped. One malformed JSON line is skipped, never fatal.
- `ChatEvent` variants, in stream order: `Session { id }`, `Text { text }`
  (append to the current reply), `ToolStart { id, name, summary, risk }`,
  `ToolEnd { id, name, ok, preview }`, `Notice { text }`,
  `Usage { prompt, completion }`, `Model { name, why }`,
  `Approval { id, tool, summary, risk, detail, always_rule, can_always, can_session, tainted }`,
  `Done { text, title }`, `Learned { items }`, `Error { message }`,
  `Cancelled`. Answer an `Approval` with `approve()`; the server declines it
  after 15 minutes if nobody answers.
- `Bridge::new(client)` owns a background tokio runtime (2 workers).
  `bridge.spawn(future)` runs work on it. `bridge.client()` returns the client.
  `bridge.handle()` returns the runtime handle for controllers that spawn
  their own tasks.
- Headless tests live in `bridge.rs` as a `#[cfg(test)]` module, against a real
  demo-mode server on a random loopback port (no provider, no network). Run
  them with `cargo test --features desktop --bin heron-desktop`. They cover the
  launch-URL parse, state, the session lifecycle, and a full chat turn.

## Threading rule (hard)

Slint widgets live on the UI thread only. Network work runs on the `Bridge`
runtime. A background task never touches a widget directly. It hands results
to the UI thread with `Weak::upgrade_in_event_loop`, and the UI-thread closure
mutates the window's models. The binary's chat wiring (`src/bin/desktop/main.rs`,
`apply_chat_event` in `models.rs`) is the worked example: stream events into
a channel on the bridge runtime, forward them one by one with
`upgrade_in_event_loop`. Never `.block_on` on the UI thread. The current
session id lives in the window (`session-id` property) precisely so it can
cross that boundary; do not keep a second copy in Rust state.

One deviation from the plan, recorded honestly: the plan prefers UI-to-backend
calls to go through a tokio `mpsc` channel with one owner. The window's
callbacks call `HeronClient` directly from `bridge.spawn` tasks instead (the
plan's own alternative shape). The callback count grew with the Chat, Views,
and Setup lanes and the direct shape still holds: each callback owns one
call and posts its result to the UI thread. If a callback ever needs
ordering against another, introduce the channel then; do not retrofit it
for its own sake.

Keyboard shortcuts (`Cmd/Ctrl+N` new conversation, `Esc` stop) and the
composer (Enter sends, Shift+Enter newline) belong to the Chat lane's
`ChatView` and `ComposerView`, not the shell. The shell only forwards the
callbacks; the spellings are the lane's unverified guesses and the buttons
cover both actions if they are wrong.

## Streaming and markdown

Each `ChatMsg` carries both the plain source text and the rendered body.
While a reply streams, the body stays plain text
(`StyledText::from_plain_text` on the accumulated text). The markdown parse
happens once, at `done` (`StyledText::from_markdown` on the full text).
Re-parsing the whole reply on every delta would be O(n) per chunk.
`apply_chat_event` in `models.rs` converts every `ChatEvent` variant into
the `ChatView` state machine: tool steps, approvals, notices, usage, and
model changes all render. Fenced code blocks, headings, lists, and tables
in `StyledText` are unverified upstream behaviour; treat them as a risk,
not a promise.

## License (settled)

Inan settled this on 2026-11-13: the royalty-free Slint route, with the
`AboutSlint` widget in the About dialog. Slint is tri-licensed (GPL-3.0-only
OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0); Heron
is Apache-2.0 and ships binaries, so the royalty-free route needs that
attribution. No paid commercial license. The About dialog lives in the shell
(`ui/main.slint`, `show-about` property, opened from the Help menu) and the
`AboutSlint` widget is inside it. The version line reads `app-version`, set
from Rust at startup, so the dialog never hardcodes the release number. The
Help-menu placement is deliberate: the `MenuItemRole` variant that would move
About into the macOS application menu is unverified, so no role is guessed.
Like everything else in this lane, the dialog has never rendered: the
`Dialog`, `AboutSlint`, and `StandardButton` shapes are unverified in this
sandbox, and the first Mac or CI build is the check for them.

## Design bar (Inan, 2026-11-13)

"Make it beautiful, more interactive, more intuitive to use." That is the bar
for the whole rebuild, on top of the settled native-look bar ("Native window,
coherent look, no webview. Slint clears it").

How the shell carries it, concretely (anti-slop guidance from the
`local-ui-excellence` skill, applied to Slint rather than the web):

- **Theme.** `ui/components/theme.slint` holds the `Theme` global: Heron's
  dark palette measured from `assets/index.html` (dark media query), serif
  headings via the web UI's serif stack. The file is a copy of the Setup
  lane's `ui/components/theme.slint` with one word added (`export`, which
  Slint requires for a global another file imports); the integration keeps
  one copy, and it must be the exported one. Do not invent a second palette: the brand hexes slate #2F6690 and
  talon #C98F00 are the light-mode values, and in dark mode they lighten to
  #7DB3D8 and #EDB92B for contrast on dark surfaces. The shell uses the dark
  values. Native widgets (Button, LineEdit, ListView, MenuBar, Dialog) keep
  the widget style's own look; custom pieces (sidebar items, the New button, the
  message bubbles) use Theme colors.
- **Motion budget.** Sidebar feedback animates over 150ms, view fades over
  180ms, easing `ease-in-out`. Nothing animates longer than ~400ms. The
  `reduce-motion` window property runs every animation at 0ms while it is on.
  Wiring it to the OS setting (or a Settings toggle) is still open; the
  property and the mechanism are done.
- **Feedback on every interactive element.** Sidebar items fade their background
  on hover and press, draw a talon ring on focus, and activate on Enter or
  Space as well as on click. The New button is slate with a talon ring on
  hover and focus, and inverts on press. The current view is unmistakable:
  slate-soft fill, semibold label, and a slate bar on the left edge. View
  switching fades the content (opacity, 180ms) instead of swapping it
  instantly. Input is gated at the branch level, not the control level:
  layouts have no `enabled` property in Slint, and opacity alone does not
  block input (only `visible: false` does that, per the Slint docs on common
  properties). So every view branch carries `visible: root.current-view ==
  "<id>"` alongside its opacity fade; a hidden view never swallows clicks
  meant for the view underneath it.
- **Sidebar rules.** New conversation is the first and loudest thing in the
  sidebar: full-width slate button, above the views. Badges read at a glance
  (talon pill, dark text): waiting approvals onto Conversation, unread inbox
  onto Reminders, unanswered suggestions onto Awareness. Totals that are not
  actions (the memory count) get no badge. The session list goes below the
  nav; the Views lane fills it, and a 30-second tick keeps the highlight on
  the conversation the chat is actually in.
- **No scaffolding left.** All nine view branches are real views now: chat,
  awareness, memory, tasks, reminders, usage, activity, onboarding, and
  settings. Each branch is the owning lane's view with the forwarded
  properties and callbacks from its contract doc.
- **Riskiest unverified syntax.** The `animate` durations are ternary
  expressions (`reduce-motion ? 0ms : 150ms`). If the Slint compiler rejects
  an expression there, the fallback is hardcoded durations and the
  reduce-motion property stops working until the wiring lands. The first Mac
  or CI build is the check, like everything else in this lane.

## Open decisions (for Inan)

None open from the Shell lane. Widget style and the license route are both
settled (see above).

## Packaging notes (for the Packaging lane)

- The Linux system libraries for the winit build are installed in CI
  (`.github/workflows/ci.yml`): `libfontconfig-dev`, `libxkbcommon-dev`,
  `libxkbcommon-x11-dev`, `libxcb-shape0-dev`, `libxcb-xfixes0-dev`,
  `libx11-dev`, `libx11-xcb-dev`, `libxcursor-dev`, `libxi-dev`,
  `libssl-dev`. The old WebView libraries are gone from the install list.
  Nothing about the Slint build was verified in this sandbox, so the first
  CI run with the `desktop` feature is the real toolchain check.
- Phase 4 lands last. Only then delete the wry/tao/muda direct deps,
  `assets/index.html` as the app UI, `build_menu()`, the webview handlers,
  and the HTML error page. Keep the server `/` route as fallback until then.

## Server endpoints (all under `/api`, all need the `x-heron-token` header)

Shapes are taken from `src/server.rs` and the old web UI (`assets/index.html`).
Errors are `{"error": "..."}` with a non-2xx status.

| Endpoint | Shape |
| --- | --- |
| `GET /state` | `{name, version, model, mode, workspace, has_key, demo, memories, unread, paused, waiting, onboarded, suggestions, mcp, usage, tools, os, sandbox}` |
| `GET /sessions` | `[{id, title, created_at, updated_at, messages}]` (newest first, 100 max) |
| `POST /sessions` | `{id}` |
| `GET /sessions/{id}` | `{id, title, messages, busy, interrupted}`. A message is `{role, content?, tool_calls: [{id, type, function: {name, arguments}}], tool_call_id?}`. |
| `DELETE /sessions/{id}` | `{ok: true}`. 409 while a reply is running there. |
| `POST /chat` | `{session_id?, message}`. SSE, see `ChatEvent`. 409 while a reply is running in that conversation. |
| `POST /approve` | `{id, decision}` where decision is `once`, `always`, `session`, or anything else to deny. |
| `POST /cancel` | `{session_id}`. Always `{ok: true}`. |
| `POST /undo` | `{session_id, force?, checkpoint?}`. `{undone, conflict?, checkpoint?, stale?}`. |
| `POST /mode` | `{mode}`. Returns `{mode}`. |
| `GET /memories[?q=]` | `[{id, kind, content, source, confirmed, uses, updated_at, ...}]` |
| `POST /memories` | `{content, kind}`. Returns `{id}`. |
| `DELETE /memories/{id}` | `{deleted: bool}` |
| `POST /memories/{id}/confirm` | `{confirmed: bool}` |
| `GET /tasks` | `[{id, title, notes, status, priority, due, owner, ...}]` |
| `POST /tasks` | `{title, due?, owner?}`. Returns `{id}`. |
| `POST /tasks/{id}` | `{title?, notes?, status?, priority?, due?, owner?}`. Returns `{updated: bool}`. |
| `DELETE /tasks/{id}` | `{deleted: bool}` |
| `GET /schedules` | `[{id, name, prompt, cron, next_run, last_run, last_tokens, last_result, quiet, trigger?, outline?, uses_model?, ...}]` (no `workflow` field) |
| `POST /schedules` | `{prompt, when, may, quiet}`. `may` is `read`, `write`, or `commands`. Returns `{id}`. |
| `POST /schedules/{id}` | `{may}`. Returns `{ok: true}`. |
| `DELETE /schedules/{id}` | `{deleted: bool}` |
| `POST /schedules/{id}/run` | `{started: true}` |
| `GET /schedules/{id}/runs` | `[{ts, status, tokens, record}]` (10 max) |
| `GET /inbox` | `[{id, title, body?, read, ts, ...}]` |
| `POST /inbox/read` | `{ok: true}` |
| `GET /audit` | `{entries, checked, intact}` |
| `GET /awareness` | `{enabled, reflect, proactive, budget_used, budget_limit, demo, focus, active_hours, branch, journal, insights, workspace}`. `proactive` is `off`, `suggest`, `prepare`, or `takeover`. |
| `POST /awareness` | `{enabled?, reflect?, proactive?}`. Returns the awareness object. |
| `DELETE /awareness` | `{removed: n}` |
| `POST /mcp/{name}/accept` | `{accepted: true, note}` |
| `GET /persona` | `{text, is_default, name, max}` |
| `POST /persona` | `{text?, name?}`. Returns the persona object. |
| `DELETE /persona` | Returns the persona object. |
| `POST /pause` | `{paused}`. Returns `{paused}`. |
| `POST /backup` | `{path}` |
| `GET /approvals` | `[{id, ...}]` pending parked approvals |
| `POST /approvals/{id}` | `{action}` where action is `approve` or `dismiss`. |
| `GET /tools` | `[{name, about, setting, always_safe}]` |
| `POST /tools/{name}` | `{setting}` where setting is `ask`, `allow`, or `never`. |
| `GET /today` | `{events, due_tasks, open_tasks, heron_tasks, waiting, unconfirmed, new_mail, unread, paused}` |
| `GET /telegram` | `{configured, paired, code?}` |
| `POST /telegram` | `{token}` (a bot token, `123456789:AA...`). |
| `DELETE /telegram` | `{configured: false, paired: false}` |
| `GET /usage` | Usage report plus `session` and `demo`, and `provider` when OpenRouter reports credits. |
| `POST /usage` | `{daily_token_limit?, monthly_token_limit?, background_tokens_per_day?, fast_model?, routing?}`. Returns the usage object. |
| `GET /sources` | `{onboarded, demo, has_model, model, about, computer, documents, calendar, email}` |
| `POST /sources/{name}` | `about`, `computer`, `documents`, `calendar`, or `email`, each with its own body (see `connect_source` in `src/server.rs`). |
| `DELETE /sources/{name}` | `{removed_items, removed_memories}` |
| `POST /sources/{name}/learn` | `{learned: [string]}` |
| `POST /onboarded` | `{ok: true}` |
| `GET /suggestions` | `[{id, prompt, session_id, ...}]` (20 max, unanswered only) |
| `POST /suggestions/{id}` | `{action}` where action is `accept` or `dismiss`. Returns `{prompt, session_id}`. |
| `GET /settings` | `{name, base_url, model, fast_model, fallback_models, key_source, provider, cli_command, cli_model, cli_config_dir, cli_allow_agent_tools, cli_allow_api_key, providers, mode, daemon_mode, auto_memory, neural_memory, neural_available, browser, browser_visible, workspace, home, allow_rules, secret_storage, mcp, mcp_changed}` |
| `POST /settings` | Same fields, all optional. `base_url: "demo"` selects the demo model. Returns the settings object. |

## Slint API notes (verified against the 1.18.1 compiler and the docs)

- **MenuItem roles.** `MenuItem` has no `role` property in the pinned Slint
  1.18.1: the compiler rejects both the property and the `MenuItemRole`
  identifier. The role API exists in the current master docs (`MenuItem.role`,
  enum `MenuItemRole`, for placing About, Settings, and Quit in the macOS
  application menu), so it arrived after 1.18.1. The Copy item carries no
  role; revisiting roles needs a Slint version bump, a separate decision.
- **StyledText wrapping.** `StyledText` has no `wrap` property (its property
  list is default-color, default-font-family, default-font-size,
  horizontal-alignment, link-color, max-lines, text, vertical-alignment).
  The `wrap` property and the `TextWrap` enum belong to the plain `Text`
  element only. `StyledText` wraps within its width without one: its
  `max-lines` doc says "Wrapped lines count towards the limit". The
  `wrap: word-wrap;` line was deleted from the chat bubble's `StyledText`;
  the three `wrap` lines on plain `Text` elements stay, and they compile.
- **Dialog is a window, not a widget.** A `Dialog` must be the root of its
  own exported component and be shown from Rust (`AboutDialog::new()`,
  `.show()`). Nested inside `MainWindow` it stays embedded in the parent
  window instead of opening a separate native window. The About dialog is
  `export component AboutDialog inherits Dialog` in `ui/main.slint`; the
  Help menu invokes the `show-about()` callback, Rust creates the dialog
  once at startup and keeps the handle alive, sets its version, and shows
  it. The OK button generates the `ok_clicked` callback only when the
  `StandardButton` is a direct child of the `Dialog`, not nested inside
  its layout; Rust hides the dialog from that callback.
- **Chat text color is explicit.** The chat bubble's `StyledText` sets
  `default-color: Theme.ink`. Without it the color comes from the widget
  palette, and under a light OS scheme that foreground is nearly black
  against the dark message backgrounds.
- **New stops the old turn two ways.** `new-session` cancels the running
  reply on the server (`POST /api/cancel`) and starts a new `ChatView`
  turn. The turn id is captured on the UI thread when the send starts and
  every SSE event carries it; `apply_event_for_turn` ignores events whose
  turn is not current, so no event from the cleared conversation can land
  in the new one (including the race between receiving and applying). The
  send handler never starts a second turn while one runs: a second send
  while a reply runs is rejected by the server (409), and the live reply's
  forwarder must keep applying.
- **Layouts have no `enabled`.** `VerticalLayout` and the other layouts take
  no `enabled` property. Input gating lives on the controls themselves
  (`Button`, `LineEdit`), which do have it. See the design-bar section.
- **Padding is layout-only.** `padding` and the `padding-*` sides belong to
  layout elements (`VerticalLayout`, `HorizontalLayout`). On a `Text` or a
  `Rectangle` the compiler warns and the padding is ignored. Three such
  paddings were replaced with layout wrappers that keep the same geometry:
  the brand wordmark and the sidebar status line each sit in a `HorizontalLayout`
  carrying the padding, and the chat bubble's `Rectangle` holds an inner
  `VerticalLayout` with `padding: 10px` around the `StyledText`. The
  warnings themselves are only warnings: the build reaches `rustc`, which
  proves the Slint stage passed.
- **Stream errors reach the UI.** `stream_chat` errors used to die silently
  (the send-message handler dropped the `Result`), leaving the pushed empty
  assistant row live and `busy` true forever. The handler now holds a clone
  of the sender and maps any `Err` into a `ChatEvent::Error`, which the
  forwarder turns into an error line and clears `busy`.
- **Learn returns an array.** `POST /sources/{name}/learn` returns
  `{learned: [string]}`, not `{learned: n}`: `learn()` in `src/sources.rs`
  returns `Result<Vec<String>>` and the route wraps it as an array.

## Backend enforcement (audited Nov 14, issue #18)

Readonly mode and approval enforcement live in the server, not in the
client, so the Slint UI cannot bypass them. The client only sends chat
text, approval decisions, and mode changes; every tool execution is
decided server-side per call.

- One decision point: `Policy::decide_in` in `src/permissions.rs` decides
  every tool call the model makes (`src/agent.rs`, the agent loop), every
  parked-approval run (`src/app.rs`, which re-decides against the current
  policy before running, so an approval granted earlier cannot execute
  under a stricter mode since), and every MCP tool call from another app
  (`src/mcp.rs`, where anything that would need approval is refused
  outright). Denials are audited (`denied-by-policy`).
- Hard rules no mode, allow rule, or setting can switch off, in order:
  catastrophic effects are denied in every mode; the user's own deny
  rules beat everything that allows; read-only denies every mutation;
  secrets always ask (Auto and Yolo included); once outside content is
  present, nothing is changed and nothing the model composed is sent out
  without approval, in every mode including Unrestricted.
- Mode changes (`POST /api/mode`) set the shared server policy; the
  client's `set_mode` and the Settings form both funnel through it.
- Coverage: `promises_that_hold_whatever_the_settings` in
  `src/permissions.rs` checks the hard rules across modes, effects,
  tainted and untainted content, and allow/deny rule sets (600+
  combinations). The approval wait declines after 900 seconds
  (`src/server.rs`).

## Open items (Shell lane)

- The window cannot run in the sandbox (no display). Verification here is
  `cargo check` without features plus the headless bridge tests in
  `src/bin/desktop/bridge.rs` (`cargo test --features desktop --bin
  heron-desktop`). Both still need a run that reaches crates.io; the desktop
  build was never verified in this sandbox. CI is the compile gate.
- Parked approvals have no UI surface: the `ViewsBridge` trait methods
  exist and the bridge implements them, but no view displays them and the
  glue never calls them.
- Undo force restore has no UI: the client exposes `undo_force` and the
  server supports it, but a conflict surfaces only as a note in the chat.
- The onboarding view has no error line on steps 1, 3, and 4, so a failed
  proactivity or finish call leaves no visible trace (the error stays in
  `state.error`). A successful Learn shows only the updated count in the
  card status, not the learned-item list the web UI shows.
- Sidebar keyboard navigation is pointer-plus-Enter/Space only.
  Arrow-key movement between sidebar items is still open.
- Wiring `reduce-motion` to the OS setting (or a Settings toggle) is still
  open. The property and the 0ms mechanism are done; nothing reads the
  platform setting yet.
- Manual launch-and-exercise of the native app on each platform, and
  release-artifact validation, are still owed before any release. CI green
  is not a substitute for running the app.
