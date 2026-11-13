# Views lane: data interface note

What the seven views need from the Shell lane, and what the Shell gets back.
Short version: the contract is the `ViewsBridge` trait in `src/ui/views.rs`.
The Shell implements it against the local HTTP server. The views never see
HTTP, tokens, or URLs.

## The contract

`ViewsBridge` is one async method per API route the views use, returning
`BridgeResult<T>` (`Result<T, String>`). The view models it returns are
plain structs in the same file, each deserializing the exact JSON the server
sends. The shapes are mapped in `docs/slint-views-map.md` and covered by
unit tests in `src/ui/views.rs`.

The plan's `HeronClient` (reqwest, one method per route) implements this
trait by delegation. `ViewsBridge` is the views-shaped subset of it. The
Shell publishes the real bridge contract. This trait does not guess it. It
states what the views need, so the Shell can check its contract against it.

## What each view sends and receives

| View | Sends (bridge call) | Receives |
| --- | --- | --- |
| Sidebar | `get_state`, `list_sessions` | badges (unread, suggestions, waiting), footer text, session rows |
| Memory | `list_memories(query?)`, `add_memory`, `delete_memory`, `confirm_memory` | memory rows with kind label, meta line, confirmed flag |
| Tasks | `list_tasks`, `add_task`, `set_task_status("done" / "open")`, `delete_task` | task rows with meta line, done flag |
| Reminders | `list_schedules`, `add_schedule`, `set_schedule_may`, `run_schedule_now`, `schedule_runs`, `delete_schedule`, `get_inbox`, `read_inbox` | schedule rows, last-run steps, delivered inbox rows (Markdown body) |
| Awareness | `get_awareness`, `set_awareness_enabled`, `set_awareness_reflect`, `set_awareness_proactive`, `clear_awareness`, `set_paused`, `list_suggestions`, `answer_suggestion` | awareness flags, proactive note, suggestion rows, right-now / insights / journal rows |

The glue in `src/ui/views_slint.rs` drives the MainWindow's forwarded API
through one `bind_window(window, ctrl)` call. It never touches the view
components directly. The Slint compiler generates Rust code only for the
last exported component of a file, so the seven view components have no Rust
types of their own. The Shell's `ui/main.slint` instantiates them and
forwards every property and callback. The glue registers the Window's
callbacks, triggers the refreshes, and posts results back through the
Window's setters. It also needs four Shell-provided closures:
`on_navigate`, `on_open_session`, `on_new_conversation`, and
`on_accept_suggestion` (accepting a suggestion hands its prompt and session
id to the Chat lane, which starts the conversation).

## Forwarding spec (what `ui/main.slint` must expose)

Window property names are `<view>-<child-prop>`. `in` child props bind
one-way (`:`). `in-out` child props bind two-way (`<=>`), so Rust clearing
a form field clears the child's input. Forwarded sets: sidebar
(reduce-motion, current-view, unread-count, suggestions-count, waiting-count, footer-text,
sessions; callbacks navigate, open-session, new-conversation); memory (reduce-motion, rows,
search-text, new-content, new-kind, empty-text; add-memory, delete-memory,
confirm-memory, search-changed); tasks (reduce-motion, rows, new-title, empty-text;
add-task, set-task-done, delete-task); reminders (reduce-motion, schedules, delivered,
schedules-empty-text, delivered-empty-text, form-error, new-what, new-when,
new-may, new-may-index, new-quiet, last-run-steps, last-run-title;
add-schedule, cancel-schedule, run-schedule-now, set-schedule-may,
show-last-run, fill-daily-briefing, fill-inbox-watch); awareness (reduce-motion, paused,
watching, reflecting, workspace, proactive, proactive-note, suggestions,
suggestions-empty-text, right-now, right-now-empty-text, insights,
insights-empty-text, journal, journal-empty-text; set-paused, set-watching,
set-reflecting, set-proactive, clear-observations, forget-insight,
answer-suggestion, open-suggestion-session); activity (reduce-motion, rows, seal-text,
seal-danger, empty-text; no callbacks, read-only); usage (reduce-motion, level-note,
today-big, today-small, today-frac, today-state, month-big, month-small,
month-frac, month-state, heading-big, heading-small, provider-visible,
provider-title, provider-big, provider-small, provider-frac,
provider-state, days, pace-note, models, models-empty-text,
models-note-visible, daily-visible, daily-rows, day-limit, month-limit,
bg-limit, bg-note, fast-model, fast-note, main-model, route-checked,
save-error, saved-visible; save-usage). NOT forwarded: `kinds` and
`may-options` (constants with defaults inside the children; the glue never
touches them).

The Shell's `HeronClient` (bridge.rs, checked Nov 13) has no usage or
audit methods yet: it covers state, sessions, and chat only. The trait
methods the views need are `get_usage`, `save_usage`, and `get_audit`
(`src/ui/views.rs`). The Shell implements them when it grows the client,
or the glue takes a second bridge that does. Nothing in the views guesses
the client shape.

The executable form of this spec is the harness test Window at
`~/workspace/slint-views-harness/ui/main.slint`. It imports the real view
files and forwards the full API. The Shell can copy it as the starting
point for `main.slint`. The harness also proves the glue against it with a
fake bridge (see `~/workspace/slint-views-harness/tests/views_glue.rs`).

One generation rule: `slint::include_modules!()` includes the generated
code at the call site, so the Window and row structs land directly at the
crate root (`crate::MainWindow`, `crate::MemoryRow`, and so on). The binary
crate's root must call it exactly once, producing those root-level types.
The glue refers to them that way and must NOT call `include_modules!()`
itself. A second call would define the same types a second time in a
different module, and the two `MainWindow` types would not match.

## Rules the Shell must hold

- Threading: the Slint event loop owns the main thread. Bridge calls run on
  the backend tokio runtime via the `Handle` passed to the controller. UI
  updates come back through `slint::invoke_from_event_loop`. Nothing ever
  blocks the UI thread.
- The proactive note is computed in `views.rs` (`ViewsState::proactive_note`).
  The Shell must not compute it a second time. It carries the demo-mode
  line and the budget line.
- Id casts: API ids are i64. Slint `int` is i32. The glue casts with `as`.
  Heron ids fit in i32 in practice. A cast that overflows would corrupt row
  identity. This is an open item, not a silent assumption.
- Errors: only the add-schedule form shows an error (the red box under the
  form). Every other bridge failure leaves the old rows in place. There is
  no error surface in the other four views.
- The inbox body renders as Markdown through `slint::StyledText`. The glue
  converts with `StyledText::from_markdown`. Fenced code blocks render
  imperfectly until the Phase 0 spike verifies them. That is the plan's open
  item, not this lane's.
- The last-run title ("Last run: Nov 13, 2:13 AM") is a Views-lane addition.
  The web UI shows the steps with no title. The steps, the status labels,
  the 160-character output cut, and the red-on-not-ok rule all mirror the
  web UI exactly.

## Open items for the Shell or Cookie

- Confirms: the web UI confirms before "take over" ("Take over lets Heron
  create and edit files in your working folder without asking first. Every
  change can be undone from its conversation. Turn it on?") and before
  "Forget everything observed" ("Forget everything Heron has observed,
  including its conclusions? What you told it directly is kept."). Slint has
  no native confirm dialog. The glue posts directly for now. The Shell must
  add a confirm step before those two calls reach the bridge.
- Parked approvals: the list renders in the Chat lane's empty state, not in
  any Views component. The bridge has `list_approvals` and
  `answer_approval` for it. The tool title map for approval dialogs belongs
  to the Chat lane. The run-step label in the glue ports only the fallback
  (strip `mcp__`, turn `__` into ` → `). One shared label table would serve
  both. That is a Shell or Chat decision.
- Palette: the `.slint` Theme carries Heron's dark palette, measured
  from the web UI's dark media query (assets/index.html lines 22-25)
  and identical to the Shell's `ui/components/theme.slint`. The
  integration keeps one copy of the Theme; until then this lane carries
  the same values so the views render dark on their own.
- Layout (CORRECTED Nov 13, measured): the brief's single `ui/views.slint`
  cannot work. The Slint 1.18 compiler generates Rust code only for the
  last exported component of a file, so six of the seven views had no Rust
  types and the glue failed with 54 errors. The per-view layout
  `ui/views/*.slint` (common, sidebar, memory, tasks, reminders,
  awareness, activity, usage) is REQUIRED by the compiler, not a
  preference. The old
  single file was deleted Nov 13 after the split verified in the harness
  (15/15 logic tests plus the behavioral glue test green). The logic in `views.rs` has no Slint dependency, so it can
  live in the library without dragging Slint in. The glue cannot. It
  belongs in the desktop binary crate next to the generated `main` module.
- Scope: the plan's Phase 2 assigns the Activity log and Usage views to
  the Views lane. They joined the lane and are built (commit e675871):
  mapped in `docs/slint-views-map.md` sections 7 and 8, drafted as
  `ui/views/activity.slint` and `ui/views/usage.slint`, wired through the
  glue, and covered by the harness behavioral test.
- Widget style per OS is SETTLED (Nov 13, Inan delegated the call to
  Cookie): cupertino on macOS, fluent on Windows and Linux, pinned per
  target in the Shell's build.rs, with Heron's dark palette (slate
  #7DB3D8, talon #EDB92B, serif headings) as custom theme properties.
  The `.slint` Theme carries those dark values. Headings use Georgia,
  the last family in the heading stack: Slint takes one family string,
  so the full comma list cannot be used here.
- Motion: every view takes `in property <bool> reduce-motion`, forwarded
  by the Window. Row hover and press highlights animate over 150ms,
  and the task title color animates when a task is toggled done. When
  reduce-motion is on, every animation runs at 0ms. The Shell owns the
  OS setting and sets the flag on the Window. Enter animations are
  deliberately absent: refreshes rebuild the row models, so a fade-in
  would replay on every load. Row clicks do nothing; the buttons inside
  each row are the actions. The Shell's rail uses the same 150ms budget
  and the same reduce-motion flag.
