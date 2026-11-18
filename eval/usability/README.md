# Heron v1 usability and live-test harness

This folder measures how Heron v1's flows feel to the people on the research-preview list,
alongside the G7 task gate in `eval/` (which it reuses, not replaces). It covers the core's HTTP
chat API today and the SwiftUI Mac app once U4 lands. Specs: `docs/designs/heron-v1.md` (API,
event stream, approvals, risk notice, permission modes, taint) and
`docs/designs/heron-mac-visual.md` (screens, keyboard paths, accessibility).

Everything is Python 3 standard library run as `python3 -I`, like `eval/`, plus two bash scripts.

## Quick start

    bash eval/usability/run.sh                       # replay the fixtures: no core, no ports; exits 0
    bash eval/usability/run.sh --live                # a core per task, demo model, this checkout's build
    bash eval/usability/run.sh --live --provider claude-cli --driver eve --all-personas
    bash eval/usability/run.sh --check               # validate tasks.md and personas.json
    bash eval/usability/mac/drive-mac.sh --plan      # the Mac app run, as a plan (U4 isn't here yet)
    python3 -I -m unittest discover -s eval/usability/tests -t eval/usability
    genesis audit-evaluator eval/usability/genesis/usability-json.yaml --suite json   # exit 0 = SOUND

`--live` needs a built core (`cargo build --locked`; the runner uses `target/debug/heron`, then
`target/release/heron`, or `--heron-bin`). It binds loopback ports, so inside Claude Code's
sandbox it needs `sandbox.network.allowLocalBinding: true`, or a run outside the sandbox.

## Files

| File | What it is |
|---|---|
| `personas.json` | Five fictional preview users, each mapped to an EVE persona and profession. |
| `tasks.md` | Ten usability tasks, one JSON block each, read with `eval/tasks_md.py`. |
| `rules.py` | The success-rule grammar and the metrics. |
| `client.py` | Starts a core, reads its endpoint, redacts the token, and runs chat, approvals and resume. |
| `run.py`, `run.sh` | The runner: replay or live, the reports, the token leak check. |
| `fixtures/recorded/` | Runs recorded live with `--record-fixtures`, with paths, ports and the token scrubbed. |
| `fixtures/synthetic/` | Runs written by hand from the spec for flows this core can't run yet. Each one says so in `_provenance`. |
| `genesis/` | The audit of the `completed` verdict: spec, verifier, case builder, cases. |
| `mac/drive-mac.sh`, `mac/gallery.py` | The Mac app driver stub and the screen gallery for EVE. |
| `tests/` | Unit tests (73). The six API tests bind a port and skip inside the sandbox. |

## Personas

All five are fictional. `surfaces` says where a persona's runs mean something.

| id | EVE persona, profession | depth | assistive | surfaces |
|---|---|---|---|---|
| `maya-dev` | developer-as-customer | high | | api, mac |
| `lena-teacher` | non-technical-user, teacher | low | | api, mac |
| `omar-keyboard` | accessibility-user | medium | keyboard-only + VoiceOver | mac |
| `priya-pm` | project-manager, executive | medium | | api, mac |
| `tom-careful` | anxious-user, accountant | low | | api, mac |

An API run tells you nothing about keyboard or VoiceOver use. When `--all-personas` runs
`omar-keyboard` over the API, its rows say `api-only` and must not be read as accessibility
coverage. That coverage comes only from the Mac app run.

## Tasks

| id | title | needs | reuses | with the demo model |
|---|---|---|---|---|
| u01 | First run and the risk notice (CUJ 2) | U3 | | yes, once U3 is in |
| u02 | A normal chat | | | yes |
| u03 | A file task that needs approval | | t01 | yes (demo variant) |
| u04 | A refused secret read (Heron's own folder) | real model | | no |
| u05 | A shell command after reading a web page: the taint re-prompt (d39430a) | real model | t07 | no |
| u06 | Switching the permission mode | | | yes (demo variant) |
| u07 | Resuming after a disconnect | U3 | t01 | yes (demo variant), once U3 is in |
| u08 | Pairing the phone (Telegram), the safe half | | | yes |
| u09 | A deletion the user is asked about | real model | t09 | no |
| u10 | Prep my day (CUJ 1) | real model | | no |

Each task has plain-language `criteria`, a machine-checkable `success` rule, a script of
`steps`, the four metrics, and a `mac` block that `drive-mac.sh` and the manual VoiceOver pass use.
Tasks that cover a core task take its prompt, approvals and pass rule from `eval/core-tasks.md`
(`say_from`, `approve_from`, `core_pass`), so nothing is copied. Heron's demo model picks a canned
tool by keyword (`DemoLlm` in `src/llm.rs`), so a core prompt would misfire there. Tasks it can
run get a `demo` variant, and the rest need `--provider <cli>`.

**u05, the taint re-prompt.** Step 0 approves a shell command "for the rest of this
conversation". Step 1 reads a fixture web page, which taints the conversation. Step 2 runs the
same command again and must be asked again. The tainted ask must offer no saved scope
(`can_session` and `can_always` false). This is the owner's 2026-11-19 decision (spec commit
`d39430a`): a tainted conversation's grants no longer reach the shell.

**u08, Telegram.** Only the safe half runs automatically: the unpaired state, and a malformed
token refused with a pointer to @BotFather. A valid token goes into the login Keychain (the
native vault, `Config::store_vault_only_secret`), which is shared with the owner's real Heron.
So full pairing is a manual step on a test bot, never a harness step.

## Modes and statuses

Each run in a report has one status:

| status | meaning |
|---|---|
| `ran-live` | The script ran against a core started for it. |
| `replayed-recorded` | Scored from a fixture recorded live earlier (`provider`, `heron_commit` in the fixture). |
| `replayed-synthetic` | Scored from a hand-written fixture. This exercises the scorer, not Heron. |
| `skipped-blocked` | A need is unmet: `U3 (core API contract) not on this core`, `needs a real model`, or no fixture yet. Never a pass. |
| `harness-error` | The harness failed: no core endpoint, a bind error, a fixture that doesn't match its script. |

In live mode the runner probes each core: `--print-endpoint` in `heron serve --help`,
`GET /api/risk-notice` and `GET /api/events` (404 means pre-U3). A task that needs `u3` is skipped on
a pre-U3 core. When the core has the route, the runner accepts the risk notice before the first
step, except in u01.

**Exit codes:** 0 when every run executed and the reports were written (an incomplete run still
exits 0 unless `--strict`); 1 on a harness error, a token found in the output, or with `--strict`
an incomplete run; 2 on a usage error or malformed task files; 3 when `--driver eve` has no `eve`.

## Options

| option | meaning |
|---|---|
| `--live` | Start a core per run (disposable `HERON_HOME` and working folder, fixture web server, `ask` mode). |
| `--provider demo\|<cli-id>` | Default `demo`. A CLI id such as `claude-cli` uses the owner's subscription. |
| `--driver direct\|eve` | `direct` sends each `say` step itself. With `eve`, `eve chat` plays the persona: it opens with the step's text and may go on for `eve_turns` turns (`--json` analysis kept per step). Live only. |
| `--tasks u01,u05` | Only these. |
| `--all-personas` | Each task once per listed persona (default: the first persona). |
| `--out DIR` | Default `eval/usability/out/<UTC time>-<mode>` (ignored by git). |
| `--record-fixtures DIR` | Live: also write a scrubbed fixture per run, for replay. |
| `--heron-bin PATH` | The core binary. |
| `--strict` | Exit 1 if any run that ran did not complete. |

## Reports

`<out>/summary.json` and `summary.md` hold one row per run: status, completion, time, asks, error
and confusion counts, and the core's capabilities. `<out>/runs/<task>-<persona>/` has
`report.json` and `report.md` (criteria, failed checks, errors, confusion points), and for live runs
`record.json` (every step's request, status, events, approval decisions and reply), `logs/` (the
core's output, token redacted), `home/` and `work/`.

### Metrics

| metric | how it is measured | audited |
|---|---|---|
| completion | The task's `success` rule over the run's evidence (`rules.evaluate`). | yes, by `genesis/` |
| time | Wall time per step, summed, and the first `text` event's delay (`ttft_ms`). | no |
| errors | `error` events (with `code`), HTTP 4xx/5xx a rule didn't expect, and transport failures. | no |
| confusion points | Asks the script didn't plan for, approvals with no summary or no detail, empty replies, cut-off streams, replies that ask for clarification (or the demo model's canned "demo mode" line), and EVE's findings and rephrase count with `--driver eve`. | no |

Time, errors and confusion points are measurements. They gate nothing and the audit doesn't
cover them. With `--driver eve`, EVE stops waiting for a reply after 30 seconds (a fixed limit
in EVE's HTTP backend), but the runner keeps the full turn. On slow CLI turns, EVE's own analysis
(its understanding score, silent misses, rephrases) can still count a reply as missing. Read it
next to `ms`.

### The token

The core's token comes only from the line the core prints, the U3 `--print-endpoint` JSON or
today's `?token=` banner. It stays in the runner process and is never put on a command line,
in an environment variable, in EVE's arguments or headers (EVE talks to an in-process bridge
that adds it), or in a file. The core's stdout and stderr go to `logs/` only after the token is
replaced by `<redacted>`; stderr lines that arrive before the endpoint is known are held back.
At the end the runner scans the whole output folder, except the disposable `home/` folders,
for every token it saw and for anything token-shaped. A hit exits 1.

## Fixtures

Recorded fixtures come from `run.sh --live --record-fixtures eval/usability/fixtures/recorded`.
The run's home, working folder and web base become `{home}`, `{work}` and `{web}`, the owner's
home becomes `~` and their user name `<user>`, and the token is redacted. A test fails if a
committed fixture still holds `/Users/`, `127.0.0.1:` or `?token=`.

Synthetic fixtures (`u01`, `u04`, `u05`, `u07`) were written by hand from the spec, so the scorer
has passing evidence for flows this core can't run yet. Replace each with a recording when its
need is met: U3 for u01 and u07, a real-model run for u04 and u05. Regenerate the audit cases
afterwards (`python3 -I eval/usability/genesis/build_cases.py`).

## The genesis audit

    genesis audit-evaluator eval/usability/genesis/usability-json.yaml --suite json

This follows U0g's pattern (`eval/genesis/`). Each dataset row is a task id, its variant and the
evidence of one run. The completion is a verdict record, exactly `{task, variant, completed,
failed_checks}`. The verifier takes the rule from `tasks.md`, never from the row. It parses the
record strictly (U0g's `strict_loads`), re-scores the evidence, and accepts only when the evidence
completes the task and the record says so. genesis only varies the completion, so the audit shows
that a verdict can't be forged, padded, swapped or smuggled. `tests/test_run.py` covers the other
half: it runs 14 known-bad cases from `cases.jsonl`, at least one per rule leaf kind, and checks
that each is rejected for the expected leaf.

Last run (2026-11-19): exit 0, **SOUND**. False-accept was 0.0% over 36 must-reject probes and
false-reject 0.0% over 9 controls. The suite bridge passed `json@0.1.0`. One `[brittle]` finding:
case and whitespace variants of the gold record are rejected, which the strict parser does on
purpose.

## The Mac app plan (U4)

`macos/HeronMac` doesn't exist on this branch yet, so the Mac run is a script stub plus this plan.
`bash eval/usability/mac/drive-mac.sh --plan` prints the steps and exits 0. Without the package,
a real run exits 3 (blocked).

Once U4 (and a bundle from U7's `packaging/macos/build-app.sh --unsigned`) exists, `drive-mac.sh`:

1. Runs U4's own snapshot tests, `swift test --package-path macos/HeronMac --filter SnapshotTests`.
   These cover the risk notice and the approval sheet in light and dark, the only pixel-compared
   screens in v1.
2. Launches `Heron.app/Contents/MacOS/Heron` directly, not with `open`, so a disposable
   `HERON_HOME` reaches the app and its core.
3. Walks the screens with the visual doc's keyboard paths: first run by its checkbox, "Accept and
   continue" and "Start using Heron", then ⌘1 to ⌘7 and ⌘,. For each screen it writes a
   screenshot (`screencapture` of the window bounds) and an accessibility dump (System Events
   `entire contents` of the window).
4. Builds a gallery (`mac/gallery.py`): `index.html` with each screenshot, its `alt` set to the
   screen's accessibility text, and `screens.md` with that text alone. Then:
   - `eve read screens.md --genre interface --persona accessibility-user` checks what a keyboard
     or VoiceOver user gets from the labels alone. **Verified here** on two of the approved
     mockups with a hand-written dump: it runs offline and returns a comprehension score and
     obstacles per block.
   - `eve run file://…/index.html` and `eve_multimodal_scan` look at the screenshots (icons,
     text in images, unlabeled images). **Not working here yet:** EVE's Playwright surface has
     no browser (`eve doctor`: run `npx playwright install chromium`). Without it,
     `eve_multimodal_scan` on the gallery returned a report of 0 screens. The script skips this
     step until `eve doctor` shows Playwright.
5. Quits the app. Its core exits when the app's pipe closes.

The terminal that runs it needs Screen Recording and Accessibility permission, and it has to
run outside Claude Code's sandbox, which blocks `osascript` and `open`. The System Events
queries are written to the visual doc but haven't run against a real window. Expect to adjust
the element names once the app exists. VoiceOver itself isn't driven. The `mac.keyboard`
checks in `tasks.md` are a manual pass, done against the accessibility dumps.

## What is blocked

| needs | blocks |
|---|---|
| U3 (core API contract) | u01 and u07 live, and the `--print-endpoint` path against a real core (it's covered by a fake-core test). |
| A real-model run (`--provider claude-cli`, owner's quota) | u04, u05, u09 and u10 live, and the EVE driver against real replies. |
| U4 (`macos/HeronMac`) plus a bundle (U7) | Everything in `mac/`. |
| EVE's Playwright browser | The screenshot scan (step 4b). |
| A test Telegram bot, by hand | Full phone pairing (u08's second half). |
