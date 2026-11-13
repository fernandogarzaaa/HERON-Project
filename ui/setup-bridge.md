# Bridge data interface: Setup lane

What each Setup view needs from the bridge, and what it sends back. The
Shell contract has landed (`docs/slint-bridge.md` in the Shell worktree,
verified against its files Nov 13): `HeronClient::from_launch_url`,
the generic `api(method, path, body)` call, typed helpers,
`stream_chat` pushing `ChatEvent` values, and `Bridge` owning a
background tokio runtime. This note is the Setup lane's implementation
of that contract.

## How the two halves fit

- `src/bin/desktop/setup.rs` holds the pure view state plus the JSON
  mapping. It uses only `std` and `serde_json`. The `SetupBridge` trait
  is async: one method per server call. The state structs are generic
  over the bridge so tests use a fake with no runtime and no server.
  `parse_*` and `*_body` are pure functions over `serde_json::Value`.
  Every server shape decision in this lane lives there, and the tests
  use payloads copied from `src/server.rs` handler code.
- `src/bin/desktop/setup_bridge.rs` is the real implementation,
  `HeronSetupBridge`. One `client.api` call per trait method, then the
  matching `parse_*`. It holds no shape logic of its own. It does not
  compile in this worktree because `HeronClient` lives in
  `src/bin/desktop/bridge.rs`, which belongs to the Shell lane. It
  compiles in the integrated tree, where `desktop.rs` declares
  `mod bridge; mod setup; mod setup_bridge;`.
- Errors come back as JSON `{"error": "..."}` with a non-2xx status.
  `HeronClient::api` turns those into an error carrying the server's
  message, and the implementation returns it as a string. The view
  shows the error text.

## Implemented surface (one trait method per call)

| Trait method | HTTP call | Sends | Receives |
|---|---|---|---|
| `load_state` | `GET /state` | nothing | `has_key, demo, onboarded, model, sandbox, os` |
| `load_sources` | `GET /sources` | nothing | full sources snapshot (see map) |
| `connect_source` | `POST /sources/{name}` | source fields | `saved` / `learned` / `indexed` per source |
| `learn_source` | `POST /sources/{name}/learn` | nothing | `{"learned": [...]}` |
| `disconnect_source` | `DELETE /sources/{name}` | nothing | `{"removed_items", "removed_memories"}` |
| `save_about` | `POST /sources/about` | 6 about strings | `{"saved": [...]}` |
| `load_proactivity` | `GET /awareness` | nothing | `proactive` field only |
| `set_proactivity` | `POST /awareness` | `{"proactive": ...}` | nothing needed |
| `use_demo` | `POST /settings` | `{"base_url": "demo"}` | nothing needed |
| `finish_onboarding` | `POST /onboarded` | nothing | `{"ok": true}` |
| `load_settings` | `GET /settings` | nothing | full settings snapshot |
| `save_settings` | `POST /settings` | settings form | full settings snapshot |
| `load_persona` | `GET /persona` | nothing | `{"text", "is_default", "name", "max"}` |
| `save_persona` | `POST /persona` | `{"text", "name"}` | persona object |
| `reset_persona` | `DELETE /persona` | nothing | default persona object |
| `load_tools` | `GET /tools` | nothing | array, only `always_safe: false` rows shown |
| `set_tool_setting` | `POST /tools/{name}` | `{"setting": ...}` | `{"name", "setting"}` |
| `accept_mcp` | `POST /mcp/{name}/accept` | nothing | `{"accepted", "note"}` |
| `load_telegram` | `GET /telegram` | nothing | `{"configured", "paired", "code"}` |
| `connect_telegram` | `POST /telegram` | `{"token": ...}` | telegram object |
| `disconnect_telegram` | `DELETE /telegram` | nothing | `{"configured": false, "paired": false}` |
| `make_backup` | `POST /backup` | nothing | `{"path": ...}` |

Two shape corrections, both read from `src/server.rs` handler code:

- `POST /sources/{name}/learn` returns `{"learned": [...]}`: an array
  of strings, not a count. `learn` in `src/sources.rs` returns a `Vec`.
  The Shell bridge doc table says `{learned: n}`. The server code says
  otherwise, and this lane uses the array. The Shell doc needs a fix.
- `GET /sources` counts are all numbers (`about.saved` is a count
  there), while `POST /sources/about` returns `{"saved": [...]}` (an
  array). The two endpoints use different shapes for the same name. The
  parsers keep them apart: `parse_sources` reads counts,
  `parse_saved` reads the array.

## Per-view data flow

### Onboarding

- Load: `load_state` + `load_sources` + `load_proactivity` together.
  Step 1 ready is `has_model && !demo`. Step 2 done is
  `about.saved > 0`. Step 3 done is any source connected. Step 4 done is
  `onboarded`.
- About form sends all six fields as strings, empty when blank. The
  result line is `Saved to memory` when `saved` is non-empty or notes is
  non-blank, else `Fill in at least one field first`.
- Source connect sends only the fields for that kind: computer sends an
  empty body, documents sends `path`, calendar sends `url`, email sends
  `host`, `username`, `password`. Email `port` (993) and `tls` (true)
  are server defaults the page never sends.
- Learn is disabled in the view when demo mode is on, or when there is
  no model and not demo. The server also refuses learn in demo mode.
- Finish posts `/onboarded` and the view starts a new conversation.

### Settings

- Load: `load_settings`, then `load_persona`, `load_tools`,
  `load_telegram`.
- Save sends the whole form in one `POST /settings`. Empty `api_key`
  keeps the stored key: the server only stores a key when the value is
  non-empty. The server rejects a non-demo `base_url` that is not a
  valid http(s) URL, and a `provider` that is neither `http` nor a
  known spec id. The response is the full settings object again, and the
  view reloads from it.
- Presets fill `base_url` and `model` in the form only. They do not save.
- Provider select: index 0 is `http` with label `API key (endpoint
  above)`. Index n + 1 is providers[n], label `{name} — {auth}`. The
  parser keeps the id list alongside the labels so a selection maps
  back to an id.
- Persona save and reset are separate from the settings save. The view
  enforces the 4000 character limit before calling the bridge. The
  server enforces it again. The settings save does NOT send `name`,
  matching the web page. The server would accept it, but the web form
  never sends it.
- Tool changes save immediately, one call per change. The view shows
  only rows where `always_safe` is false. The parser filters the rest.
- MCP lines: the parser splits each `mcp` line at the first colon to get
  the app name, and marks `changed` when the name is in `mcp_changed`.
  Accept posts and returns the note. The view shows
  `{name}: accepted. {note}`.
- Telegram connect sends the token and clears the input on success. The
  server validates the token shape before storing it. Disconnect returns
  no note, so the view shows its own disconnected text.
- Backup returns a full path. The view shows only the file name.

## Step flow and motion

Design direction (Inan, Nov 13): "make it beautiful, more interactive,
more intuitive to use." Anti-slop basis: `local-ui-excellence`
(`SKILL.md`, `references/anti-slop.md`, `references/web.md`). Applied
here as: no gradients, no glass, no heavy shadows, no decorative
numbering (the steps are genuinely sequential, so numbering them encodes
real order), no generic hero, real copy from the web UI, motion that
communicates rather than decorates.

- One step visible at a time. The indicator shows the four steps with
  done (✓, slate), current (talon, bold), and upcoming (muted) states,
  plus a fill bar whose width animates when a step completes.
- Flow state lives in Rust (`OnboardingState.current_step`). The view
  shows the step and fires `goto_step(n)`. Back is always allowed.
  Forward requires every step in between to be passable, and only the
  model step gates: about and world are both skippable, as in the web UI.
- "Save and continue" advances from step 2 to step 3 only on a real
  save. An empty save stays put and shows the message.
- `progress` is the fraction of steps done (0.0 to 1.0), computed by
  Rust from the same done conditions as the indicator.
- Motion, aligned with the shell's budget: step fades 180ms, progress
  fill 180ms, card selection 150ms. Nothing animates longer than 180ms.
  The fade uses the `opacity` + `visible: opacity > 0.01` pattern on
  each step section. That pattern is unverified until the Slint
  toolchain lands, like the rest of the `.slint` here.
- Reduced motion: the views take the window's `reduce-motion` property
  (default false), passed down from `MainWindow`. When true, every
  duration above is 0ms and the view cuts instantly. Wiring that
  property to the OS setting is still open (Shell lane note).
- One deliberate divergence from the shell: the shell's view branches
  overlap inside a fixed Rectangle, so they fade with `opacity` alone.
  My step sections sit in a flow `VerticalBox`, where an opacity-0
  section would still occupy layout space. That is why the steps also
  need `visible: opacity > 0.01` to collapse the space.
- Settings keeps the web UI grouping (model, personality, permissions,
  per-tool, connected apps, phone, memory and files) because that order
  already puts each control where the user expects it. The polish there
  is visual only: dark theme, serif headings, pressed feedback on the
  choice cards.

## Wiring to the Shell contract (integration spec)

`ui/main.slint` and `src/bin/desktop.rs` are Shell-owned. This is the
exact surface integration needs from this lane. Nothing here edits
Shell files.

### Modules

`src/bin/desktop.rs` declares, next to the existing modules:

```rust
mod setup;
mod setup_bridge;
```

`setup.rs` uses only `std` and `serde_json`. `setup_bridge.rs` uses
`super::bridge::HeronClient`, `reqwest::Method`, and `super::setup::*`.
Both live behind the `desktop` feature with the rest of the binary.

### Window: branch replacement

`ui/main.slint` has scaffolding branches for `onboarding` and
`settings`. Each branch becomes the lane's view, imported at the top
of `main.slint`:

```slint
import { OnboardingView } from "views/onboarding.slint";
import { SettingsView } from "views/settings.slint";
```

- The `onboarding` branch becomes `OnboardingView` with
  `reduce-motion: root.reduce-motion`, the same `opacity` fade and
  `enabled: root.current-view == "onboarding"` the scaffolding branch
  already has.
- The `settings` branch becomes `SettingsView` the same way.
- On startup, when the loaded state says `onboarded` is false, the
  driver sets `current-view` to `"onboarding"`. Otherwise it stays
  `"chat"`.
- The rail already has `onboarding` and `settings` items. No rail
  change is needed.

### Driver: one pattern per callback

The driver follows the exact pattern `desktop.rs` already uses for
the chat callbacks: install the callback on the window, spawn the work
on the Bridge runtime, hand the result to the UI thread with
`Weak::upgrade_in_event_loop`. Never `.block_on` on the UI thread.
Sketch for one action (`save_about`); every other callback has the
same shape, with its own trait method and its own state mutation on
the UI thread:

```rust
// The driver holds OnboardingState<HeronSetupBridge> and
// SettingsState<HeronSetupBridge> beside the window.
let bridge = bridge.clone();
let weak = window.as_weak();
window.on_save_about(move || {
    let client = bridge.client().clone();
    bridge.spawn(async move {
        let result = HeronSetupBridge::new(client)
            .save_about(&form_snapshot)
            .await;
        let _ = weak.upgrade_in_event_loop(move |window| {
            // Apply `result` to the view state, then copy the state
            // fields back into the window properties.
        });
    });
});
```

The full callback list the driver installs for this lane, with the
trait method each one calls:

| View callback | Trait method |
|---|---|
| `open_settings()` | (switches `current-view` to `"settings"`, no call) |
| `use_demo()` | `use_demo` then `refresh` |
| `save_about()` | `save_about` then `refresh` |
| `connect_source(kind)` | `connect_source` then `refresh` |
| `learn_source(kind)` | `learn_source` then `refresh` |
| `disconnect_source(kind)` | `disconnect_source` then `refresh` |
| `set_proactivity(level)` | `set_proactivity` |
| `goto_step(n)` | pure state change, no call |
| `finish()` | `finish_onboarding` then `refresh` |
| Settings `save()` | `save_settings` |
| Settings `save_persona()` | `save_persona` |
| Settings `reset_persona()` | `reset_persona` |
| Settings `set_tool(name, setting)` | `set_tool_setting` |
| Settings `accept_mcp(name)` | `accept_mcp` |
| Settings `connect_telegram(token)` | `connect_telegram` |
| Settings `disconnect_telegram()` | `disconnect_telegram` |
| Settings `make_backup()` | `make_backup` |
| `apply_preset(preset)` | pure form change, no call |

The `refresh` after each mutating call reloads state, sources, and
proactivity (onboarding) or the settings snapshot (settings), so the
view always shows what the server now holds.

### Folder picker (not a server call)

The documents step uses a real folder picker instead of the web UI's
typed path (plan section 4). Slint has no native file dialogs.
`rfd` 0.18 is declared behind the `desktop` feature; wiring is still
open. The interface:

- The `.slint` side fires a `pick_folder_requested()` callback from the
  documents `SourceCard` (`Choose folder` button).
- The driver opens `rfd::AsyncFileDialog` inside `slint::spawn_local`
  and feeds the picked path back through
  `OnboardingState::set_document_path`, via `upgrade_in_event_loop`.
  A cancelled picker calls nothing, and the previous path stays.
- The picked path is then sent as `path` in `POST /sources/documents`,
  exactly as the web UI sent the typed path.
- The picker is not part of `SetupBridge`. It is UI-side plumbing owned
  by the desktop binary, not a server call.

## Things the bridge does NOT provide

- Provider install and login status. The server has no endpoint for it.
  `GET /settings` returns the 16 CLI specs with id, name, auth, cost,
  login command, and experimental flag, but no status. The web page
  tells the user to run `heron providers`. If the Slint build wants
  status in Settings, that is a Shell or CLI decision, not a Setup one.
- Live chat, sessions, tasks, reminders, usage, activity, awareness
  journal, or suggestions. Those belong to the Chat and Views lanes.
- The mode buttons in the top bar (`POST /mode`). The Settings form
  saves `mode` through `POST /settings`. The top bar is the Shell lane.

## Open items

- `src/bin/desktop/setup.rs` and `src/bin/desktop/setup_bridge.rs`
  are not declared as modules yet. The declarations in
  `src/bin/desktop.rs` belong to Shell and integration (see the spec
  above).
- `setup_bridge.rs` does not compile in this worktree (no
  `HeronClient` here). Verified here with `rustfmt --check` only. The
  first Mac or CI build of the `desktop` feature is the real check.
- The `.slint` files are uncompiled. No Slint toolchain exists in this
  sandbox yet (plan section 9: `index.crates.io` times out through the
  proxy). Compiling them waits on the Shell lane toolchain with Slint
  pinned `=1.18.1` and `slint-build =1.18.1`. No build is claimed
  verified until a `cargo build` exits 0 with the pinned versions on a
  machine with crates.io access or in CI. The fade pattern
  (`opacity` + `visible: opacity > 0.01`) is unverified until then.
- Style: all three plan decisions are settled. Widget style is cupertino
  on macOS and fluent on Windows and Linux, pinned per target in build.rs
  by the Shell lane. `ui/components/theme.slint` carries Heron's dark
  palette, measured from the web UI dark-mode values, plus the serif
  heading stack. No style selection lives in this lane's files. The
  shell's copy of the theme is byte-identical to this lane's file; the
  integration keeps one copy.
- `reduce-motion` wiring to the OS setting is still open (Shell lane
  note). The views take the window's property; the property defaults
  to false.
- Shell doc correction owed: `docs/slint-bridge.md` says
  `POST /sources/{name}/learn` returns `{learned: n}`. The server
  returns an array (`src/server.rs:965`, `src/sources.rs` `learn`).
  This lane uses the array.
