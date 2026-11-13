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
from the endpoints mapped below); onboarding and Settings (Setup lane). Activity log and Usage joined this
lane in Phase 2 (plan section 6) and are mapped in sections 7 and 8 below.

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

- The Today block in the empty chat view: `GET /today` returns
  `{ "events": [string], "due_tasks": [{ "id", "title", "due",
  "overdue": bool }], "open_tasks", "heron_tasks", "waiting",
  "unconfirmed", "new_mail", "unread", "paused" }`. Chat lane owns the
  rendering; the `due_tasks` and `unread` data overlap my Tasks and
  Reminders views.

## 7. Activity log (`#v-activity`, nav "Activity log")

HTML: `h2` "Activity log", intro "Every action Heron took or was refused,
newest first. Passwords and keys are blanked out before anything is
logged.", then `#audit`. No form, no buttons. Read-only.

### API

- `GET /api/audit` (server `get_audit`, server.rs:1014). Returns
  `{ "entries": [AuditEntry], "checked": <int>, "intact": <bool> }`.
  `checked` is the number of entries the seal verification checked.
  `intact` is true when no entry was altered or removed outside Heron.
  The plan's word "seal" is this `checked`/`intact` pair. Up to 200
  entries (`audit_log(200)`), newest first.
- `AuditEntry` (store.rs:144): `{ "ts": <unix seconds>, "session_id":
  <string>, "tool": <string>, "args": <string>, "decision": <string>,
  "result": <string> }`. `args` is a display string, already redacted.

### Display rules (loadActivity, index.html:1010)

- Empty list: one line, "Nothing yet. Actions appear here as Heron
  works." No seal line, no table.
- Otherwise the seal line comes first: intact ->
  "The log is intact: {checked} entries checked, none altered or
  removed." with the count formatted with thousands separators; not
  intact -> "Warning: this log has been altered outside Heron. Entries
  may have been changed or removed." in the danger color.
- Table columns: When, Action, Details, Outcome. One row per entry.
  When is `fmtTime(ts)` ("Nov 13, 2:13 AM"). Action is the tool label
  (`label(tool)`): the `toolLabel` map belongs to the Chat lane, so the
  glue ports only the fallback (strip `mcp__`, turn `__` into ` → `).
  Details is `args` in monospace. Outcome maps the decision:
  `allowed` -> "Done", `approved` -> "You allowed", `approved-always`
  -> "You allowed (always)", `denied-by-user` -> "You didn't allow",
  `denied-by-policy` -> "Blocked", anything else shows as it is. A
  decision starting with `denied` renders the Outcome cell in the
  danger color.

## 8. Usage (`#v-usage`, nav "Usage")

HTML: `h2` "Usage", intro "How many tokens Heron has used, how many are
left of the limits you set, and where this month is heading. Tokens are
what your AI model provider counts and charges for.", a level note
(`#u-level`), tiles (`#u-tiles`), "Last 30 days" with a bar chart
(`#u-days`) and a pace note (`#u-pace`), "This month by model"
(`#u-models`), "Your limits" with three number inputs (`#u-day`,
`#u-month`, `#u-bg`), "Choosing a model" with a cheaper-model input
(`#u-fast`) and a routing checkbox (`#u-route`), a "Save limits" button
(`#u-save`) and a "Saved" indicator (`#u-saved`).

### API

- `GET /api/usage` (server `get_usage`, server.rs:813). Returns the
  `Report` (usage.rs, "Everything the Usage page shows") serialized,
  plus `session` (the session token totals), `demo` (bool), and an
  optional `provider` object (only when the base URL contains
  openrouter.ai and the credits call succeeds).
- `Report`: `{ "budget": Budget, "level_label": <string>, "days":
  [[<day "YYYY-MM-DD">, <tokens u64>] x 30, oldest first, zero days
  included], "models": [[<model>, <in u64>, <out u64>, <calls u64>]],
  "main_model": <string>, "fast_model": <string>, "routing": <bool>,
  "background_today": <u64>, "background_limit": <u64> }`.
- `Budget`: `{ "level": "normal" | "careful" | "minimal" | "spent",
  "today": Window, "month": Window, "daily_average": <u64>,
  "projected_month": <u64>, "limit_reached_on": <"YYYY-MM-DD" or null> }`.
  `level_label` is "Normal", "Economy", "Strict economy", or "Allowance
  used up". `Window`: `{ "used": <u64>, "limit": <u64, 0 means no limit>,
  "left": <u64 or null> }` (`left` is null when there is no limit).
  `daily_average` is the average over the last seven days, zero days
  counted. `limit_reached_on` is the day the monthly limit would be
  reached at the recent pace, or null.
- `provider`: `{ "name": "OpenRouter", "credits_bought": <f64>,
  "credits_used": <f64>, "credits_left": <f64> }`. Money, not tokens.
- `POST /api/usage` (server `set_usage`, server.rs:836). Body:
  `{ "daily_token_limit": <u64>, "monthly_token_limit": <u64>,
  "background_tokens_per_day": <u64>, "fast_model": <string>,
  "routing": <bool> }`. Numbers or digit strings (commas, spaces, and
  underscores stripped server-side). Empty means 0, which the server
  reads as no limit. Returns the fresh `GET /usage` payload. Setting
  `routing` false stores `"off"`; true stores `"auto"`.

### Display rules (loadUsage, index.html:969)

- Tiles, in order. "Today": big `tok(today.used)`, small
  `"{tok(left)} left of {tok(limit)}"` or "No limit set" when there is
  no limit. "This month": same shape from the month window. "Heading
  for": big `tok(projected_month)`, small: with a monthly limit and a
  `limit_reached_on`, "Limit reached around {Mon D}"; with a monthly
  limit and none, "Within your monthly limit"; without a monthly limit,
  "by the end of this month, at the recent pace". Provider tile, only
  when `provider` exists: title `"{name} credit"`, big
  `"$X.XX left"` (two decimals) when `credits_bought > 0`, or "None";
  small `left of $Y.YY bought` or "No credit bought on this account, so
  only free models will answer." A tile with a limit shows a meter: width
  is `min(100, round(used / limit * 100))` percent. The meter is `out`
  when `left == 0`, `low` when `left / limit <= 0.25`, plain otherwise.
- Level note (`#u-level`): demo ->
  "Demo mode uses no tokens. Connect a model to see real figures.";
  `normal` -> empty; `careful` -> "Economy mode is on: a quarter or less
  of your allowance is left, so Heron is taking fewer steps, keeping
  answers short and doing nothing on its own."; `minimal` -> "Strict
  economy is on: your allowance is almost used up. Heron is using its
  cheapest model and the least work that answers you."; `spent` ->
  "Your allowance is used up. Heron still answers you, as cheaply as it
  can, and has stopped everything it does on its own. Raise the limit
  below to lift this."
- 30-day chart: one bar per day, oldest first, height
  `max(n ? 3 : 0, round(n / max * 100))` percent where `max` is the
  largest day (at least 1). The last bar (today) is highlighted. The
  hover title is `"{Mon D}: {N} tokens"`. The plan says the Slint
  version is custom-drawn rectangles, not a chart library. The bars show
  figures only on hover, so the web UI also renders a "Daily figures"
  details table (newest first, Day and Tokens columns) right after the
  pace note. The Slint version keeps that table behind a show/hide
  toggle, since Slint has no `<details>` element.
- Pace note: `daily_average` non-zero ->
  "About {tok(daily_average)} tokens a day over the last week. Hover a
  bar for the day's figure; today is highlighted."; zero ->
  "Nothing used yet."
- Models table: columns Model, Sent to it, Written by it, Calls. Rows
  from `models`, numbers with thousands separators. Empty ->
  "No model calls this month." When rows exist, a note follows: "Where a
  provider does not report token counts, Heron estimates them from the
  length of the text."
- Limits form: the three inputs show the current limits as plain digits
  (empty for no limit). The background note reads "Suggestions,
  preparing ahead, taking over and reflection. {N} used today. Reminders
  you set up yourself always run." The cheaper-model input shows
  `fast_model` (empty when none). The routing checkbox is checked when
  `routing` is true and disabled when there is no fast model or the fast
  model equals the main model. Its note reads "Your main model is
  {main_model}. Name a cheaper one here to let Heron choose between
  them."
- Save: the button posts the form. Each number input is stripped of
  non-digits; empty reads as 0. `routing` is sent as `fast ? (route
  disabled ? true : route checked) : false`. On success the "Saved"
  indicator shows for 1.8 seconds, the usage reloads, and the sidebar
  state refreshes. On failure the web UI alerts the error message. The
  Slint version shows the error as red text under the button instead,
  since Slint has no alert dialog.
- `tok(n)`: `n >= 1e6` -> `X.XM` (one decimal, `.0` stripped; zero
  decimals at `>= 1e7`); `n >= 1e4` -> `Xk` (rounded); otherwise the
  number with thousands separators. Already ported as `tok` in
  `src/ui/views.rs`.
