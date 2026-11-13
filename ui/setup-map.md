# Setup lane UI map: onboarding, settings/providers, persona

Source of truth: `assets/index.html` at base `ea06729a`, plus the route
handlers in `src/server.rs`. Every API call below goes through
`fetch('/api' + path)` with an `x-heron-token` header. All shapes below are
measured from that code, not inferred.

## Shared: GET /state

Used by onboarding (model readiness, onboarded flag) and settings
(sandbox and OS for the sandbox note).

Response: `name, version, model, mode, workspace, has_key, demo, memories,
unread, paused, waiting, onboarded, suggestions, mcp, usage, tools, os,
sandbox`. `has_key` is true when an API key exists, the base URL is local,
or demo mode is on. `sandbox` is a name such as `"none"`.

## 1. Onboarding (`#v-onboarding`, nav "Getting to know you")

Shown automatically at startup when `state.onboarded` is false. Four steps
in one ordered list. A step shows a done marker when its condition holds.
The Slint build shows one step at a time with an indicator and a fill bar
(see the step-flow section in `ui/setup-bridge.md`). The Rust side owns
`current_step` and the `goto_step` guard: back is always allowed, forward
past the model step needs a model or demo mode. About and world are
skippable.

### Step 1: Connect an AI model (`#ob-model`)

Loads `GET /sources` and `GET /awareness` together.

- Ready condition: `sources.has_model && !sources.demo`.
- Status text when ready: `Connected to {sources.model}.`
- Status text in demo: `You are in demo mode: Heron can act, but it cannot
  think or learn yet. Connect a model whenever you are ready.`
- Status text otherwise: `Heron needs an AI model to think with. Add an API
  key, use a model running on this computer, or look around in demo mode
  first.`
- Button: `Connect a model` (or `Change model` when ready). Opens Settings.
- Second button when not ready and not demo: `Use the demo for now`.
  `POST /settings` with body `{"base_url": "demo"}`, then reloads.

### Step 2: Tell Heron about yourself (`#ob-about`)

Form `#ob-about-form`, fields: name, work, goals, style (single-line
inputs), people (textarea, one per line), notes (textarea).

- Submit: `POST /sources/about` with body
  `{"name","work","goals","style","people","notes"}`, all strings, empty
  when blank.
- Response: `{"saved": ["..."]}`, a list of memory statements Heron wrote.
- Done condition: `sources.about.saved > 0`.
- Saved line: `Heron remembers {n} thing(s) you told it`.
- Result text after submit: `Saved to memory` when `saved` is non-empty or
  the notes field is non-blank, else `Fill in at least one field first`.

### Step 3: Connect your world (`#ob-world`)

One card per source, in this order: computer, documents, calendar, email.
Each card is built by `sourceCard(name, status, info)` from `GET /sources`.

`GET /sources` response:

```json
{
  "onboarded": false, "demo": false, "has_model": true, "model": "gpt-5.5",
  "about": {"saved": 0, "indexed": 0, "learned": 0},
  "computer": {"connected": true, "indexed": 12, "learned": 3},
  "documents": {"connected": false, "folders": [], "indexed": 0, "learned": 0},
  "calendar": {"connected": false, "hosts": [], "indexed": 0, "learned": 0},
  "email": {"connected": false, "address": null, "indexed": 0, "learned": 0}
}
```

Card fields and actions per source (from the `SOURCES` table in the page):

| source | title | fields on the connect form | connect button |
|---|---|---|---|
| computer | This computer | none | `Look around` |
| documents | Notes and documents | no typed field; a `Choose folder` button opens a real folder picker | `Add folder` (`Add another folder` when connected) |
| calendar | Calendar | `url` (Private calendar link, https://... .ics) | `Connect calendar` (`Add another calendar` when connected) |
| email | Email | `host` (Mail server), `username` (Email address), `password` (App password) | `Connect email` |

- Connected status line: `Connected. ` plus `{indexed} recent messages /
  events / files indexed on this computer` with `from {address}` for email,
  `from {folders}` for documents, `from {hosts}` for calendar, and
  `Heron learned {n} thing(s) from it.` when `learned` is set. Computer
  reads `Heron knows its way around this computer.`
- Connect submit: `POST /sources/{name}` with the form fields as JSON.
  Email also accepts optional `port` (default 993) and `tls` (default true);
  the page never sends them. Response is `{"learned": [...]}` for computer,
  `{"indexed": n}` for documents, calendar, and email.
- Documents folder: the Slint build replaces the web UI's typed path with
  a real folder picker (plan section 4). The view fires
  `pick_folder_requested`. The desktop driver opens
  `rfd::AsyncFileDialog` inside `slint::spawn_local` and feeds the picked
  path back into the form through `set_document_path`. A cancelled picker
  changes nothing. The picked path is then sent as `path` in the connect
  call, exactly as the web UI sent the typed path.
- Email host auto-fill: typing a username fills `host` from the domain when
  it matches gmail.com, googlemail.com, icloud.com, me.com, fastmail.com,
  or yahoo.com, and only when `host` is still empty.
- `Learn from it` button, shown when connected and the source is not
  computer: `POST /sources/{name}/learn`, response `{"learned": [...]}`.
  Disabled in demo mode, and disabled when there is no model and not demo.
  Fails server-side in demo mode with `Demo mode cannot learn from your
  information. Connect a model first.`
- `Disconnect and forget` button, shown when connected: confirm dialog,
  then `DELETE /sources/{name}`, response
  `{"removed_items": n, "removed_memories": n}`.
- A result list or error line is shown under the card after each action.
- Done condition for the step: any of the four sources is connected.
- Extra card after the four: `Other apps`. It is text only: Heron can use
  any app that offers an MCP connector; add the connector to the settings
  file in the Heron folder and its tools appear after a restart. No API call.

### Step 4: Decide how much Heron does on its own (`#ob-pro`)

Radio group `#ob-level`, values `off`, `suggest`, `prepare`, `takeover`.
Labels: `Only when I ask`, `Suggest work`, `Prepare ahead`, `Take over`.
The checked value comes from `GET /awareness` field `proactive`.

- Change: `POST /awareness` with body `{"proactive": value}`. The `takeover`
  choice asks for confirmation first; declining reloads the current value.
- Done condition: `sources.onboarded` is true.
- Finish button `#ob-finish`: `POST /onboarded` with an empty body,
  response `{"ok": true}`, then refresh state and start a new conversation.
  Label is `Finish setup`, or `Back to the conversation` when already
  onboarded.

## 2. Settings (`#v-settings`)

Loads `GET /settings`, then `GET /persona` and `GET /tools` in parallel.
The form `#settings` saves with one `POST /settings`.

### GET /settings response

```json
{
  "name": "Heron", "base_url": "https://openrouter.ai/api/v1",
  "model": "anthropic/claude-sonnet-5.5", "fast_model": "",
  "fallback_models": "gpt-5.5, llama3.3",
  "key_source": "saved",
  "provider": "http", "cli_command": "", "cli_model": "",
  "cli_config_dir": "", "cli_allow_agent_tools": false, "cli_allow_api_key": false,
  "providers": [{"id": "claude-cli", "name": "Claude Code (subscription)",
    "auth": "Claude Pro/Max subscription", "cost": "...",
    "login": "claude auth login", "experimental": false}],
  "mode": "ask", "daemon_mode": "auto",
  "auto_memory": true, "neural_memory": false, "neural_available": true,
  "browser": "Google Chrome", "browser_visible": false,
  "workspace": "/home/user", "home": "/home/user/.heron",
  "allow_rules": [], "secret_storage": "the system keychain",
  "mcp": ["slack: ..."], "mcp_changed": []
}
```

- `key_source` is `environment`, `saved`, or `none`.
- `fallback_models` is one comma-separated string, not an array.
- `providers` holds 16 CLI specs (claude-cli, codex-cli, cursor-cli,
  grok-cli, opencode-cli, antigravity-cli, cline-cli, copilot-cli,
  qwen-cli, gemini-cli, pi-cli, fx-cli, kiro-cli, muse-cli, goose-cli,
  custom-cli). The select shows each as `{name} — {auth}`. There is no
  install or login status in this response. The page only says to run
  `heron providers` to check install and login status.
- `browser` is the browser name string, or null when no supported browser
  was found.

### AI model section

Fields: Service address (`base_url`), Model (`model`), Provider
(`provider`, select), CLI command (`cli_command`), CLI model
(`cli_model`), API key (`api_key`, password input), Backup models
(`fallback_models`), Model for background work (`fast_model`).

- Preset buttons fill `base_url` and `model` only, and do not save:
  OpenRouter (`https://openrouter.ai/api/v1`,
  `anthropic/claude-sonnet-5.5`), OpenAI (`https://api.openai.com/v1`,
  `gpt-5.5`), Ollama on this computer (`http://localhost:11434/v1`,
  `llama3.3`), Demo (`demo`, `demo`).
- Provider select starts with one option, `API key (endpoint above)`
  with value `http`. The 16 CLI providers are appended once, from
  `GET /settings` field `providers`.
- API key input is always cleared on load. Placeholder is `Paste your key`
  when `key_source` is `none`, else `Leave empty to keep the current key`.
- Key note under the field: demo base URL gives `Demo mode needs no key.`;
  `environment` gives `A key is already set through an environment
  variable on this computer.`; `saved` gives `A key is saved in
  {secret_storage}. It is never shown again or sent anywhere except to the
  service above.`; `none` gives `Kept in {secret_storage}. Not needed for
  models running locally, like Ollama.`
- Save sends: `base_url, model, fallback_models, fast_model, provider,
  cli_command, cli_model, api_key, mode, daemon_mode, auto_memory,
  neural_memory, browser_visible, workspace`. Empty `api_key` means keep
  the stored key. The server rejects a non-demo `base_url` that is not a
  valid http(s) URL, and a `provider` that is neither `http` nor a known
  spec id. The response is the full settings object again.

### Personality section (GET /persona, POST /persona, DELETE /persona)

Fields: What to call it (`name`, max 40 characters), Character (`text`,
mono textarea).

- `GET /persona` response: `{"text": "...", "is_default": false,
  "name": "Heron", "max": 4000}`.
- `POST /persona` body: `{"text": "...", "name": "..."}`. The server
  rejects text over 4000 characters. Response is the persona object again.
- Save button (`#s-persona-save`) is separate from the main settings save.
  It shows `Personality saved` on success.
- Reset button (`#s-persona-reset`): confirm dialog, then
  `DELETE /persona`. Response is the default persona object. The button
  is hidden while `is_default` is true.
- The name change is also the agent name in settings; the main settings
  save does not send `name`.

### What Heron may do without asking

- Mode radios (`readonly`, `ask`, `auto`, `yolo`), saved by the main
  settings save as `mode`. The top bar in the chat view uses the same
  values through `POST /mode` with `{"mode": ...}`.
- `During scheduled reminders` select (`daemon_mode`): `readonly` or
  `auto`.
- Sandbox note (`#s-sandbox`): when `state.sandbox` is set and not `none`,
  the sandboxed text; on Windows without a sandbox, the Windows text; on
  Linux without one, the bubblewrap text.
- `Show the browser while Heron uses it` checkbox (`browser_visible`).
  Disabled when `settings.browser` is null. Note text differs on whether
  a browser was found.

### Each ability, one by one (GET /tools, POST /tools/{name})

- `GET /tools` returns an array of
  `{"name", "about", "setting", "always_safe"}`. The page shows only
  rows where `always_safe` is false.
- Each row has a select with `ask` (Ask as usual), `allow` (Never ask),
  `never` (Never allow).
- Change sends `POST /tools/{name}` with `{"setting": ...}` immediately.
  Response: `{"name": ..., "setting": ...}`.

### Connected apps (MCP)

- `settings.mcp` is a list of strings, each starting with the app name
  before the first colon. Shown as plain lines. When no apps are
  connected, the text is `No apps connected through MCP.`
- When an app name appears in `settings.mcp_changed`, the line is shown
  in red with an `Accept the changed tools` button. Confirm dialog, then
  `POST /mcp/{name}/accept`, response
  `{"accepted": true, "note": "Restart Heron for the change to take
  effect."}`. The line then reads `{name}: accepted. {note}`.

### Use Heron from your phone (GET /telegram, POST /telegram, DELETE /telegram)

- `GET /telegram` response:
  `{"configured": false, "paired": false, "code": null}`. `code` is set
  only when configured and not yet paired.
- Status text: paired gives `Paired with your phone. Only that chat is
  answered. ...`; configured but unpaired gives `Open your bot in
  Telegram and send it this code to pair your phone: {code}. Until then it
  answers nobody.`; unconfigured gives the @BotFather instructions.
- Connect: `POST /telegram` with `{"token": "..."}`. The server rejects
  anything that is not `{digits}:{20 or more letters, digits, _ or -}`.
  The token input is cleared on success.
- Disconnect: confirm dialog, then `DELETE /telegram`. Response is
  `{"configured": false, "paired": false}` and carries no `note` field,
  even though the page reads one. The Slint view must show its own
  disconnected text instead of the missing field.

### Memory and files

- `Learn about me from conversations` checkbox (`auto_memory`).
- `Understand meaning when searching memory` checkbox (`neural_memory`).
  The row is hidden when `neural_available` is false. The server only
  stores true when neural search is built in.
- `Working folder` input (`workspace`). Changes apply after restarting
  Heron.
- Home note: `Conversations, memory and settings are stored in {home}.`
- `Save a copy of my data` button: `POST /backup`, response
  `{"path": "..."}`. Shows `Saved in the backups folder as {file name}`.
  A copy holds conversations, memory, tasks, reminders, automations, and
  the activity log. Restore is `heron restore` from a closed Heron.
- Save button for the whole form shows `Settings saved` on success, or
  the error text in `#s-err` on failure. After a save the page reloads
  settings and refreshes state.

## 3. Persona, as its own piece

Persona lives inside Settings in the web UI, but it has its own endpoints
and its own save and reset buttons, so the Slint build keeps it as a
separate `PersonaEditor` component used by `SettingsView`. Its contract
is the `/persona` GET, POST, and DELETE above, with the 4000 character
limit and the 40 character name limit.
