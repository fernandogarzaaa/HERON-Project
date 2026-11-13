# Slint Views: UI map of the web UI

Source of truth: `assets/index.html` (1074 lines, read at base `ea06729a`) and
`src/server.rs` (1017 lines) plus the store types in `src/store.rs`.
Every API call below was read from the frontend JavaScript, and every JSON shape
was read from the matching server handler. Nothing here is inferred from the
shape of a name alone.

Conventions the web UI uses, and the native UI keeps:
- Every call goes to `/api` + path with the `x-heron-token` header.
  The token comes from the launch link once, then lives in session storage.
- Errors arrive as `{ "error": "..." }` with a non-2xx status. The frontend
  throws `new Error(data.error || "Request failed (N)")`.
- Times are Unix seconds. The frontend formats them as
  `Nov 13, 6:46 PM` (`toLocaleString` with month short, day numeric,
  hour 2-digit, minute 2-digit).
- The sidebar refreshes state every 30 seconds while no reply is in flight.
- Lists are newest first where the server orders them that way; the frontend
  does not re-sort.

## Lane boundaries

Mine (Views lane): the sidebar rail, the recent-conversations list, Memory,
Tasks, Reminders (including the Delivered inbox section), Awareness
(including the suggestions list and the parked-approvals display).

Not mine: the conversation view and composer (Chat lane); the empty-state
blocks that render Today, waiting approvals, and offers inside the chat view
(Chat lane owns the rendering, but the approvals and suggestions data come
from the endpoints mapped below); onboarding and Settings (Setup lane);
Usage and Activity log (unassigned in the lane briefs, not mapped here
beyond a one-line pointer each).

Shared: `GET /state` feeds my sidebar badges and the Shell's mode buttons
and footer. The Shell owns view switching (`show(name)`).

## 1. Sidebar rail

HTML: `.rail` aside, 264 px wide, full height. Brand row (Heron mark + wordmark),
a full-width "New conversation" button, a `nav` with nine section buttons,
a "Recent conversations" heading, the sessions list, and a footer line.

### Nav buttons and badges

| Button | View | Badge source |
| --- | --- | --- |
| Conversation | chat | `state.waiting` (count, id `wait-n`) |
| Getting to know you | onboarding | none |
| Awareness | awareness | `state.suggestions` (count, id `offers-n`) |
| Memory | memory | none |
| Tasks | tasks | none |
| Reminders | reminders | `state.unread` (count, id `unread`) |
| Usage | usage | none |
| Activity log | activity | none |
| Settings | settings | none |

The active button gets `aria-current="page"`. Badges hide when the count is 0.
Footer text: `{model} — {session tokens} tokens this session`, or
`Demo mode` in place of the model name when `state.demo` is true.
Session tokens are `state.usage.prompt_tokens + state.usage.completion_tokens`.

### Recent conversations

`GET /sessions` returns `[SessionInfo]`, newest first (server order).
The frontend keeps only entries with `messages > 0` and shows
`title || "Untitled"`, with the full timestamp as a hover tooltip.
Clicking one opens it: `GET /sessions/{id}` (Chat lane owns the opened view).
The current session gets `aria-current="true"`.

```json
// GET /sessions -> [SessionInfo]
{ "id": "01K...", "title": "Plan the Rust course", "created_at": 1763000000,
  "updated_at": 1763001000, "messages": 12 }
```

`GET /sessions/{id}` returns `{ "id", "title", "messages": [...], "busy": bool,
"interrupted": bool }`. The message shape belongs to the Chat lane.
`DELETE /sessions/{id}` returns `{ "ok": true }`, and fails with 409 while
a reply is running in that session. `POST /sessions` returns `{ "id" }`
and is called by the Chat lane on first send, not by the sidebar.

### GET /state (sidebar data source)

```json
{ "name": "Heron", "version": "0.2.0", "model": "anthropic/claude-sonnet-5.5",
  "mode": "ask", "workspace": "/home/fernando/work", "has_key": true,
  "demo": false, "memories": 42, "unread": 2, "paused": false,
  "waiting": 1, "onboarded": true, "suggestions": 3,
  "mcp": ["slack: 12 tools"], "usage": { "prompt_tokens": 1200, "completion_tokens": 800 },
  "tools": 34, "os": "macos", "sandbox": "none" }
```

Fields the sidebar uses: `model`, `demo`, `unread`, `suggestions`, `waiting`,
`usage.prompt_tokens`, `usage.completion_tokens`, `paused` (the empty state
shows a paused banner; the Awareness view shows the pause toggle).

## 2. Memory view (`#v-memory`)

Intro line: "What Heron knows about you. It stays in a file on this computer,
and you can remove anything."

### Calls

- `GET /memories` or `GET /memories?q=<text>` (search input, debounced 200 ms)
  returns `[MemoryItem]`, at most 500 without a query, 50 with one.
- `POST /memories` with `{ "content": "...", "kind": "fact" }` returns `{ "id": 7 }`.
  Content shorter than 3 characters is rejected server-side.
- `DELETE /memories/{id}` returns `{ "deleted": true }`.
- `POST /memories/{id}/confirm` returns `{ "confirmed": true }`.

```json
// MemoryItem
{ "id": 7, "kind": "fact", "content": "I am vegetarian and allergic to peanuts",
  "tags": "", "importance": 0.7, "created_at": 1763000000,
  "updated_at": 1763000000, "uses": 3, "source": "user", "confirmed": true }
// search results add: "score": 0.93
```

### Display rules (read from `loadMemory`)

- Kind labels: `profile` -> "About me", `preference` -> "Preference",
  `workflow` -> "How I like things done", `project` -> "Project",
  `person` -> "Person", `fact` -> "Fact". The add form offers the same six
  kinds, default `fact`.
- Confirmed item meta line: `{source label}, {formatted updated_at}`.
  Source labels: `user` -> "You added this", `onboarding` -> "You told Heron
  while getting set up", `agent` -> "Heron saved this during a conversation",
  `auto` -> "Heron picked this up from a conversation". Anything else falls
  back to "Saved".
- If `uses == 0` and the item is older than 30 days, the meta line appends
  ". Not used in over a month".
- Unconfirmed item meta line: `Picked up from {origin}, not confirmed by you.
  Heron treats it as possibly wrong.` Origin labels: `email` -> "your email",
  `calendar` -> "your calendar", `documents` -> "your documents",
  `observed` -> "watching your folder", `computer` -> "looking around this
  computer". Unconfirmed items get a green "Confirm" button beside "Forget".
- Empty states: no query and no items -> "Nothing remembered yet. Tell Heron
  about yourself, or add something above." With a query ->
  `Nothing in memory matches "{q}".`
- After add, delete, or confirm, the list reloads and `GET /state` refreshes
  (the memory count in the footer changes).

## 3. Tasks view (`#v-tasks`)

Intro line: "Your to-do list. Heron can add to it and tick things off when
you ask."

### Calls

- `GET /tasks` returns `[Task]`.
- `POST /tasks` with `{ "title": "..." }` returns `{ "id": 3 }`.
  Optional fields the server accepts: `due` (string, empty means none) and
  `owner: "heron"`.
- `POST /tasks/{id}` with `{ "status": "done" }` or `{ "status": "open" }`
  returns `{ "updated": true }`. The server also accepts `title`, `notes`,
  `priority` (integer), `due`, and `owner`.
- `DELETE /tasks/{id}` returns `{ "deleted": true }`.

```json
// Task
{ "id": 3, "title": "Call the dentist", "notes": "", "status": "open",
  "priority": 2, "due": "2026-11-14", "created_at": 1763000000,
  "owner": "user" }
```

### Display rules (read from `loadTasks`)

- Each row is a checkbox, the title, and a meta line. Done rows are struck
  through and muted.
- Meta line joins, with " — ": `"Heron is handling this"` when
  `owner == "heron"`, `"Due " + due` when `due` is set, and `notes` when
  non-empty. Empty meta renders nothing.
- Toggling the checkbox posts the new status and reloads the list.
- Empty state: "No tasks yet. Add one above, or ask Heron to."

## 4. Reminders view (`#v-reminders`)

Intro line explains scheduled runs, automations, and the read-only default for
unattended runs. Two sections: "Scheduled" and "Delivered".

### Calls: schedules

- `GET /schedules` returns `[Schedule]` with three computed fields added by
  the server: `trigger`, `outline`, `uses_model`. The raw `workflow` JSON
  string is removed from the response.
- `POST /schedules` with `{ "prompt", "when", "may": "read"|"write"|"commands",
  "quiet": bool }` returns `{ "id": 5 }`. `when` accepts a time phrase
  ("in 30m", "tomorrow 9:00", "daily 08:00", "every 2h") or an event phrase
  ("when notes/*.md changes", "when a file changes in Drafts",
  "when an email arrives from Maria").
- `POST /schedules/{id}` with `{ "may": "read"|"write"|"commands" }`
  returns `{ "ok": true }` (changes what an unattended job may do).
- `POST /schedules/{id}/run` returns `{ "started": true }`. The frontend
  then polls `GET /schedules` every 1.5 s for up to 90 s until `last_run`
  advances past the previous value, then reloads.
- `GET /schedules/{id}/runs` returns up to 10 runs:
  `{ "ts", "status", "tokens", "record": { "steps": [{ "what", "status",
  "output" }], "error": "..." } }`.
- `DELETE /schedules/{id}` returns `{ "deleted": true }`.

```json
// Schedule (as returned by GET /schedules)
{ "id": 5, "name": "Morning briefing", "cron": "0 8 * * *",
  "prompt": "Give me a short morning briefing...",
  "enabled": true, "next_run": 1763085600, "last_run": 1762999200,
  "last_result": "Done", "allow": "", "quiet": false, "last_tokens": 1240,
  "trigger": null, "outline": null, "uses_model": null }
```

Notes on the fields:
- `cron` is empty for one-shot jobs and for trigger jobs. `trigger` is a
  human sentence only when `cron` starts with `@` (`@file:**` ->
  "When any file in your folder changes", `@file:<glob>` -> "When <glob>
  changes", `@email` -> "When a new email arrives", `@email:<needle>` ->
  "When an email arrives from or about \"<needle>\""). Otherwise `trigger`
  is null and the row shows the cron in words (see below).
- `outline` is a description string when the job is an automation with steps,
  else null. `uses_model` is true/false only for automations.
- `allow` is the granted permission string: `""` (read only),
  `"write_file:*\nedit_file:*"` (write), or that plus `"shell:*"` (commands).
  The `may` value in requests maps: `read` -> `""`, `write` ->
  `"write_file:*\nedit_file:*"`, `commands` -> `"write_file:*\nedit_file:*\nshell:*"`.

### Display rules (read from `loadReminders`)

- Only `enabled` schedules are listed. Each row shows the `name` for
  automations (`outline` set) or the `prompt` for plain jobs.
- Meta line: `{trigger sentence}` when present, otherwise
  `{cron in words}. Next: {formatted next_run}` for recurring jobs, or
  `Once, {formatted next_run}` for one-shot jobs. Then ` — {may text}` where
  the may text is "read only", "may edit files", or
  "may edit files and run commands" (from `allow`). Then `, speaks up only
  when needed` when `quiet` is true. Then, when `last_run` is set:
  `. Last ran {time}` plus `, used {tokens} tokens` when `last_tokens` is
  non-zero, plus `: {last_result}` when quiet and a result exists.
- Automations show a kind tag: "Automation, uses the AI model" or
  "Automation, no AI model needed", and the outline in a monospace block.
- Row controls: "Run now" button, a permission dropdown (read / write /
  commands, pre-selected from `allow`), and for automations with a last run,
  a "Show last run" button that loads `GET /schedules/{id}/runs` and renders
  the newest run's steps as `"{label} — {status}{: output first 160 chars}"`.
  Step status labels: `ok` -> "done", `failed` -> "failed",
  `waiting` -> "waiting for you". A run error renders in red.
- Empty state: "Nothing scheduled."
- Cron in words (ported from `cronText` in the frontend; the Rust port lives
  in `src/ui/views.rs`): "Every day at 9:00 AM", "Every weekday at 8:00 AM",
  "Every Saturday and Sunday at 10:00 AM", "Every Monday at 9:00 AM",
  "On day 15 of every month at 9:00 AM", "Every 30 minutes",
  "Every 2 hours", "Every hour at 15 minutes past". Anything else renders
  as `Repeats (0 8 * * *)`.

### Calls: delivered inbox

The "Delivered" section lists results of scheduled runs.

- `GET /inbox` returns `[InboxItem]`, newest first, at most 100.
- `POST /inbox/read` returns `{ "ok": true }`. The frontend calls it when
  the Reminders view loads and any item is unread, then refreshes state
  (the Reminders badge clears).

```json
// InboxItem
{ "id": 9, "ts": 1763000000, "title": "Morning briefing", "body": "## ...",
  "read": false }
```

The body is Markdown and renders as formatted text. Meta line is the
formatted timestamp. Empty state: "Results of scheduled runs appear here."

### Add-reminder form

Fields: what (`rem-what`), when (`rem-when`), may (`rem-may`, read/write/
commands), quiet checkbox (`rem-quiet`). On submit the form posts
`POST /schedules` and clears on success; on error it shows the server error
message in a red box. Two fill-in buttons preset the form:
- "Fill in a daily briefing": what = "Give me a short morning briefing: my
  open tasks by priority, anything due today, and what I was working on most
  recently in my folder.", when = "daily 08:00".
- "Fill in an inbox watch": what = "Look at the new email. If it needs a
  reply from me today, tell me who it is from, what they need and a
  suggested reply. Do not reply yourself.", when = "when an email arrives",
  quiet checked.

## 5. Awareness view (`#v-awareness`)

Intro line: "While Heron is open, it quietly watches your working folder so
it already knows what you are in the middle of. It sees file names and times,
never what is inside, and never passwords or key files. Everything it notices
stays on this computer."

### Calls

- `GET /awareness` returns the awareness object (below).
- `POST /awareness` with `{ "enabled": bool }`, `{ "reflect": bool }`, or
  `{ "proactive": "off"|"suggest"|"prepare"|"takeover" }` returns the full
  awareness object again.
- `DELETE /awareness` returns `{ "removed": <count> }`. It clears observations
  and deletes every memory whose source is `observed`. The frontend asks for
  confirmation first: "Forget everything Heron has observed, including its
  conclusions? What you told it directly is kept."
- `POST /pause` with `{ "paused": bool }` returns `{ "paused": bool }`.
  Pausing stops reminders, watches, suggestions, and background checks.
- `GET /suggestions` returns up to 20 open suggestions (see below).
- `POST /suggestions/{id}` with `{ "action": "accept" }` returns
  `{ "prompt": "...", "session_id": "..." }`. With `{ "action": "dismiss" }`
  it records the dismissal. Accepting starts a conversation with the prompt
  (Chat lane owns that step).
- `GET /approvals` returns parked approvals (see below). The list renders in
  the Chat lane's empty state, and the count feeds the Conversation badge.
- `POST /approvals/{id}` with `{ "action": "approve" }` returns
  `{ "result": "..." }` (the parked action runs). With `{ "action": "dismiss" }`
  it returns `{ "dismissed": true }`.

```json
// GET /awareness
{ "enabled": true, "reflect": true, "proactive": "suggest",
  "budget_used": 1200, "budget_limit": 5000, "demo": false,
  "focus": [["src", 14], ["docs", 3]], "active_hours": [9, 18],
  "branch": "main", "workspace": "/home/fernando/work",
  "journal": [{ "ts": 1763000000, "path": "src/server.rs" }],
  "insights": [ /* MemoryItem, source "observed", at most 20 */ ] }
```

### Display rules (read from `renderAwareness`)

- Pause checkbox: checked when `state.paused` is true (the value comes from
  `GET /state`, not from the awareness object). Label: "Pause everything
  Heron does on its own. Reminders, watches, suggestions and background
  checks all stop until you switch this off. From your phone, send /pause
  or /resume."
- "Watch my folder in the background" checkbox maps to `enabled`, and shows
  "Watching {workspace}". "Think about what it notices" maps to `reflect`,
  disabled while `enabled` is false.
- Proactivity radio group, four levels, each with a description:
  `off` "Only when I ask", `suggest` "Suggest work",
  `prepare` "Prepare ahead", `takeover` "Take over". The radios disable
  while `enabled` is false. Choosing `takeover` asks for confirmation:
  "Take over lets Heron create and edit files in your working folder without
  asking first. Every change can be undone from its conversation. Turn it on?"
- Note under the radios: the default text explains that suggesting, preparing,
  and taking over send file names, tasks, and memory to the model about every
  half hour while there is new activity, which uses tokens. In demo mode with
  a level other than off: "Demo mode cannot come up with suggestions.
  Connect a model in Settings." When a background budget limit exists and the
  level is not off, the note appends today's usage: "Today Heron has used
  {used} of its {limit} token allowance for work it starts itself", plus
  ", so it is paused until tomorrow" when the budget is spent.
- "Heron suggests" section renders the open suggestions (below).
- "Right now" section: when watching is off, one line: "Background watching
  is off. Heron only knows what you tell it." Otherwise up to three rows:
  "Your focus this week" (`"{area} ({n} changes)"` joined with ", ", or
  "Nothing has changed in the folder yet."), "Git branch" (when a branch is
  known), "Your usual hours" ("Mostly active between {start}:00 and
  {end}:00" or "Not enough history yet. Heron needs a few days before it
  says anything about your habits.").
- "What Heron has concluded" lists insights as memory rows with a "Forget"
  button (`DELETE /memories/{id}`). Empty states: no insights and reflect on
  and not demo -> "Nothing yet. Heron reflects after it has seen enough
  activity, at most every six hours." Reflect on and demo ->
  "Demo mode cannot reflect. Connect a model to get insights." Reflect off ->
  "Turn on 'Think about what it notices' to let Heron draw conclusions."
- "Recently noticed" lists the journal, at most 40 entries, each as
  `{path}` with meta `Changed {formatted ts}`. Empty states: watching on ->
  "Nothing noticed yet. Changes in your folder appear here within a minute."
  Watching off -> "Nothing is being recorded."
- "Forget everything observed" button at the bottom (see `DELETE` above).

### Suggestions

```json
// Suggestion
{ "id": 4, "ts": 1763000000, "title": "Draft the invoice follow-up",
  "why": "You edited invoices.md three times today.",
  "prompt": "Draft a follow-up email for the unpaid invoice...",
  "status": "new", "result": null, "session_id": null }
```

Status values: `new`, `prepared`, `done`, `failed`, `accepted`, `dismissed`.
Title prefix by status: `done` -> "Done for you: ", `prepared` -> "Prepared: ",
`failed` -> "Could not finish: ", otherwise none. `why` renders as a muted
line, `result` renders as Markdown when present. Buttons: `new` gets "Do it"
(accept, then the Chat lane starts a conversation with the returned prompt);
when `session_id` is set, "Continue in a conversation" (or "Review what
changed" when done) opens that session; the dismiss button reads "Not now"
for new, "Got it" for done, "Dismiss" otherwise. The Awareness view shows up
to 20; the Chat lane's empty state shows up to 3.

### Parked approvals

```json
// ParkedApproval
{ "id": 11, "ts": 1763000000, "session_id": "01K...", "tool": "write_file",
  "args": "{\"path\": \"notes.md\"}", "summary": "Write notes.md",
  "detail": "Write 12 lines to notes.md", "risk": "normal",
  "tainted": false, "origin": "Morning briefing", "status": "pending",
  "result": null }
```

Rendered (in the Chat lane's empty state) as "Waiting for you", at most 5,
each with the tool title, `While working on "{origin}", {time}. It has not
been done.` plus a tainted warning when `tainted` is true, the detail in a
monospace block, and "Do it now" / "Don't do it" buttons. The tool title
comes from the same title map the Chat lane uses for approval dialogs
(`write_file` -> "Heron wants to write a file", and so on).

## 6. Pointers to the views that are not mine

- Usage view: `GET /usage` returns the budget object (tiles, 30-day sparkline,
  per-model table, limits form). `POST /usage` saves limits. Owned by no
  lane in the briefs; the Setup lane is the closest owner.
- Activity log: `GET /audit` returns `{ "entries": [AuditEntry], "checked",
  "intact" }`. AuditEntry: `{ "ts", "session_id", "tool", "args", "decision",
  "result" }`. Decision labels: `allowed` -> "Done", `approved` ->
  "You allowed", `approved-always` -> "You allowed (always)",
  `denied-by-user` -> "You didn't allow", `denied-by-policy` -> "Blocked".
  Also unassigned.
- The Today block in the empty chat view: `GET /today` returns
  `{ "events": [string], "due_tasks": [{ "id", "title", "due",
  "overdue": bool }], "open_tasks", "heron_tasks", "waiting",
  "unconfirmed", "new_mail", "unread", "paused" }`. Chat lane owns the
  rendering; the `due_tasks` and `unread` data overlap my Tasks and
  Reminders views.
