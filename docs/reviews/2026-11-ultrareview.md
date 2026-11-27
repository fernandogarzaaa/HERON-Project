# Heron v1 review, improvements and upgrade plan (2026-11-22)

* **Target:** PR #62 (Heron v1, squash-merged into `main` as `f732b4e`), plus the open fix PRs #70 and #71.
* **Code read:** `main` at `39dbdf5`. Only docs changed after `f732b4e`, so every `file:line` below is on `main`.
* **Status:** proposals only. No code was changed for this review.

## How this review ran

* **Level.** The owner asked for an *ultra* review. Ultra (the deep cloud review) isn't enabled for this account yet: the skill said to run `claude auth login` to refresh entitlements. The skill fell back on its own to a local **max**-effort review. Max is one level above the xhigh fallback that was planned.
* **Shape.** 63,590 added lines are too many for one reviewer, so the review split by code area. Each area had a finder that applied five correctness angles: line by line, removed behaviour, callers and callees, language pitfalls, and wrappers.
  * The areas were the security core, the trust boundary (memory, sources, Telegram, MCP), the HTTP API, the agent harness, Swift HeronCore, Swift HeronUI, and the eval gates.
  * One more finder looked at removed behaviour across the whole PR.
  * One looked for cleanup, efficiency, altitude and CLAUDE.md conventions in the biggest Rust files.
  * Then one gap sweep looked only for defects the others missed. It added #16, #17 and five lows.
  * #70 and #71 each had their own finder.
* **Verdicts.** I checked every finding I rank against the code myself and gave it a verdict:
  * **CONFIRMED:** I can name the input and the wrong result.
  * **PLAUSIBLE:** the mechanism is real, but the trigger is uncertain.
  * Anything a finder raised that I didn't check myself is marked *finder-only*.
* **Already tracked, so not reported again:**
  * the "Known low issues" section of `docs/designs/heron-v1.md`;
  * the G3 records;
  * draft PRs #63 to #67;
  * the owner-reported gaps that are being fixed elsewhere: Connect buttons, upgraders skipping v1 setup, the sidebar footer, #70 and #71.
* **Disclosure.** The repository is public, so seven security findings (#1, #2, #6, #7, #13, #16, #17) give only severity, impact, area and effort here. Their mechanisms, `file:line` and fixes are held locally and were given to the owner.

## Summary

| | High | Medium | Low (kept) |
|---|---|---|---|
| PR #62, ranked findings | 1 | 16 | see "Other candidates" |
| PR #70 | 0 | 1 (CONFIRMED live) | 3 |
| PR #71 | 0 | 2 (PLAUSIBLE) | 4 |

**The headline.** Three of the top findings let content the owner didn't write get around a trust rule:

1. The web-fetch guard (#1).
2. Taint marking (#2).
3. Memory trust (#3): undoing "Forget" in the Mac app brings an outside memory back as user-confirmed.

All three are small, local fixes. They matter beyond memory and the web. When outside text doesn't taint the conversation (#2) or gets in as trusted memory (#3), the taint rule never engages, and Auto mode's normal allowances still apply. The shell rule holds only for conversations that are correctly marked as tainted.

## Part 1: PR #62 findings, most severe first

### 1. HIGH, CONFIRMED: sub-agent delegation can weaken the web-fetch guard in a tainted conversation

* **Impact:** In a conversation that already holds outside content, one path lets a model-chosen fetch run without the approval it should need.
* **Area:** `src/agent.rs` (delegation).
* **Effort:** 1-2 h.
* **Details:** held locally and shared with the owner, because this repository is public.

### 2. MEDIUM, CONFIRMED: one class of MCP tool result bypasses taint marking

* **Impact:** Some third-party text from an MCP server reaches the model without the conversation being marked tainted, so the taint rules don't engage.
* **Area:** `src/agent.rs`, `src/mcp.rs`.
* **Effort:** 1 h.
* **Details:** held locally and shared with the owner, because this repository is public.

### 3. MEDIUM, CONFIRMED: Undo after "Forget" re-adds an outside memory as user-confirmed

* **Where:** `macos/HeronMac/Sources/HeronUI/Stores/ContentStores.swift:181-186`; `src/server.rs:874-879` (`save(content, kind, "", 0.7, "user")`).
* **Mechanism.** The app's `undoForget()` restores a forgotten memory by calling `POST /api/memories` with its text. That route always saves with source `user`. A row that came from outside content, unconfirmed and low-trust, comes back as the owner's own confirmed fact. `edit()` is a forget plus an add, so it does the same to whatever text it keeps.
* **Owner invariant:** "memory never trusts outside content". An undo click isn't vouching for the content.
* **Fix.** Delete softly, with a short-lived `forgotten_at` column, and add a `POST /api/memories/{id}/restore` that puts the row back exactly as it was. For edit, keep the row's source and add an explicit "I confirm this" step.
* **Effort:** half a day, across `src/server.rs`, `src/store.rs` (a migration) and `ContentStores.swift`.

### 4. MEDIUM, CONFIRMED: the envelope decoder panics on a backslash before a non-ASCII character

* **Where:** `src/cli_llm.rs:1103` (`other => out.push(other as char)`, then `k += 2`), panicking at `src/cli_llm.rs:1108` (`text[k..]`).
* **Mechanism.** For an unknown escape (`\é`, `\’`), the decoder pushes one byte and moves past two. `k` lands inside a multi-byte character, and the next `text[k..]` panics. It runs on every streamed fragment in envelope mode, which covers claude-cli and codex-cli.
* **Result.** Model output with a stray backslash before an accented letter (LaTeX, a Windows path, French) kills the turn. Through #5, it also leaves the conversation stuck as busy.
* **Fix.** For an unknown escape, copy the whole next character and advance by its UTF-8 length. Add a test.
* **Effort:** 1 h.

### 5. MEDIUM, CONFIRMED: a panic inside a chat run leaves the conversation busy and the stream open

* **Where:** `src/server.rs:554-560` (spawn) and `src/server.rs:596-617` (`run_chat`).
* **Mechanism.** The conversation is freed (`st.finish`) and `run_end` is published only when `run_turn` returns. A panic anywhere in the turn skips both. Panics include #4, the slices in "Other candidates" and #10. The session stays in `running`, so every new message gets `session_busy` until Stop or a restart, and the open SSE stream never ends.
* **Fix.** Wrap the run in a guard whose `Drop` calls `finish` and publishes `run_end` with an error. Alternatively, `catch_unwind` the turn future. Then sweep `src/` for byte slices (Improvement 2).
* **Effort:** 2-3 h.

### 6. MEDIUM, CONFIRMED: Auto mode's destructive-command patterns can be sidestepped

* **Impact:** Some destructive commands are classed as ordinary commands, so Auto mode runs them without asking.
* **Area:** `src/permissions.rs` (shell patterns).
* **Effort:** 2-3 h.
* **Details:** held locally and shared with the owner, because this repository is public.

### 7. MEDIUM, CONFIRMED (design trade-off): sandboxed commands can change files that run later outside the sandbox

* **Impact:** Some writable build folders hold code the owner runs later, outside Heron. Owner decision needed: tighten, hash-check (Upgrade 2) or accept in `docs/ACCEPTED_RISKS.md`.
* **Area:** `src/sandbox.rs` (writable folders).
* **Effort:** 1 h, or see Upgrade 2.
* **Details:** held locally and shared with the owner, because this repository is public.

### 8. MEDIUM, CONFIRMED: an approved action is cancelled if the app's request times out

* **Where:** `src/server.rs:1175-1180` (awaits `run_parked` inside the HTTP handler), `src/app.rs:1763-1770` (claims the approval, setting it to `running`), `macos/HeronMac/Sources/HeronCore/CoreClient.swift:14` (`timeoutIntervalForRequest = 60`).
* **Mechanism.** The approved tool call runs inside the request. If it takes more than 60 s, for example a build, `URLSession` gives up and closes the connection. axum then drops the handler future, which cancels the tool call partway and leaves the approval in `running`. `recover_interrupted_approvals` runs only when a process takes the background lock, which normally happens at startup.
* **Result.** The owner approves a long command, sees an error, and the record says it's still running.
* **Fix.** Run the parked call on its own task: return `202` with the approval id, and send the result over SSE. Alternatively, spawn it and await its `JoinHandle`, so a dropped request no longer cancels it.
* **Effort:** half a day, across `src/server.rs` and `ChatAndApprovalsStores.swift`.

### 9. MEDIUM, CONFIRMED: triggers and proactive work ignore the single-owner lock

* **Where:** `src/app.rs:1819-1831` (`observer_loop`), compared with `src/app.rs:2134` (`scheduler_loop` checks `own_background_work()`). Spawned at `src/server.rs:225` and `src/main.rs:848`.
* **Mechanism.** Only the scheduler checks `background.lock`. The observer loop runs `sources::sync_due`, `reflect_if_due`, `run_triggers` and `proactive_tick` in every Heron process on the same home.
* **Result.** With the app open and a terminal `heron serve` or daemon also running, email and file triggers fire twice, and proactive messages and model spending double.
* **Fix.** Run the background parts of `observer_loop` only when `own_background_work()` is true. Ambient observation can stay per-process.
* **Effort:** 1 h.

### 10. MEDIUM, CONFIRMED: Today view panics on a task whose due date has a multibyte character

* **Where:** `src/server.rs:1225` and `:1230` (`d[..d.len().min(10)]`).
* **Mechanism.** `due` is free text from the `task_add` tool (`src/tools/personal.rs:127`, written by the model) or from `POST /api/tasks` (`src/server.rs:895`). Neither checks the date format. A character that crosses byte 10 panics the handler.
* **Result.** `/api/today` fails until someone edits the task.
* **Fix.** Validate `due` when it's written (`YYYY-MM-DD[ HH:MM]`), and compare with `d.get(..10)` instead of slicing.
* **Effort:** 1 h.

### 11. MEDIUM, CONFIRMED (test quality): CI lets the Seatbelt tests skip and stay green

* **Where:** `.github/workflows/ci.yml:26` and `release.yml:75` run `cargo test` without `HERON_REQUIRE_SEATBELT=1`. Only `scripts/verify-v1.sh:43` sets it. The check itself is at `src/sandbox.rs:522-527`.
* **Mechanism.** The sandbox tests skip when `sandbox-exec` is unusable, for example on a runner image that blocks it. Only the gate turns a skip into a failure, so CI can show green for sandbox tests that never ran.
* **Fix.** Set `HERON_REQUIRE_SEATBELT: "1"` on the macOS jobs. The eval tests (`eval/tests`, `eval/containment`) also don't run in CI (finder-only). Add a `uv run pytest eval` step.
* **Effort:** 1-2 h.

### 12. MEDIUM, PLAUSIBLE: the audit hash chain can fork when two Heron processes write

* **Where:** `src/store.rs:1312-1321`.
* **Mechanism.** `audit_row` reads the last hash, then inserts in a separate statement. They aren't in one `BEGIN IMMEDIATE` transaction, and the lock is per process. With the app's core and a terminal `heron` writing at the same time, both can chain from the same previous row. `verify_audit` then reports tampering that never happened.
* **To confirm:** a two-process test that writes from both at once.
* **Fix.** Wrap the read and the insert in `BEGIN IMMEDIATE`.
* **Effort:** 1-2 h.

### 13. MEDIUM, PLAUSIBLE: a Seatbelt gap could let a sandboxed command use the owner's credentials indirectly

* **Impact:** A class of local connection isn't restricted by the profile. Needs a Seatbelt test to confirm.
* **Area:** `src/sandbox.rs` (profile).
* **Effort:** 2-4 h.
* **Details:** held locally and shared with the owner, because this repository is public.

### 14. MEDIUM, PLAUSIBLE: the approval card for task and memory edits hides what would be written

* **Where:** `src/tools/personal.rs:401-410`: `task_update`'s summary is `#id status`, and `detail` is `None`. `task_add` and `forget` are similar (finder).
* **Mechanism.** In a tainted conversation these writes ask, as designed. But the card doesn't show the new title or notes. The owner approves blind, and outside text then lands in tasks, which are later shown in trusted context.
* **Fix.** Add a `detail` that shows the fields being written.
* **Effort:** 1 h.

### 15. MEDIUM, PLAUSIBLE (evaluator): the G7 code verifier imports the candidate into the process that runs the tests

* **Where:** `eval/genesis_verifier.py:37-50`.
* **Mechanism.** The nonce is read after the import, but the candidate's import-time code runs in the same interpreter as the hidden tests. It can patch builtins, `json.load` or `sys.stdin` before any test runs.
* **Fix.** Run `genesis audit` on this verifier, per the owner's rule for evaluators. Run the tests in a fresh interpreter that imports the candidate only inside each test.
* **Effort:** 2-3 h.

### 16. MEDIUM, CONFIRMED (gap sweep): a crafted incoming email can crash mail sync

* **Impact:** One message can make the sync task panic on every pass.
* **Area:** `src/mail.rs`.
* **Effort:** 30 min.
* **Details:** held locally and shared with the owner, because this repository is public.

### 17. MEDIUM, CONFIRMED (gap sweep): a safety switch for the browser tool reads its setting incorrectly

* **Impact:** One configuration value turns a guard off when it should leave it on.
* **Area:** `src/browser.rs`.
* **Effort:** 15 min.
* **Details:** held locally and shared with the owner, because this repository is public.

### Other candidates (lower, or finder-only)

**Reliability, Rust:**
* Gap sweep, low: `parse_duration` and `in N unit` overflow on huge numbers (`src/schedule.rs:134`).
* Gap sweep, low: `json_string_field` byte-slices after a backslash (`src/ui/chat.rs:414`, Slint app).
* Gap sweep, low: lossy casts on the email port (`as u16`) and `proactive_interval_mins * 60`.
* Gap sweep, low: the `ambient_context` flag also turns off source sync, triggers and proactive work (`src/app.rs:1821`).
* Gap sweep, low: tests race on process-wide env vars (`HERON_API_KEY`) with no lock and no restore.
* `text[a..=b]` on model output with no `b >= a` check: `src/app.rs:1869`, `src/app.rs:2000`, `src/memory.rs:519`, `src/sources.rs:502`. These panic the observer loop's step when a `]` comes before the first `[`. CONFIRMED as code, low.
* `host_only` panics on a Host header like `[::1` with no closing bracket, before the token check (`src/server.rs:~99-102`). Finder-only, low.
* `parse_trigger` mixes byte offsets from the lowercased string and the original (`src/schedule.rs:178`). Finder-only, low.

**Mac app UX** (finder-only, medium per the finder):
* `open(id:)` and `startNewConversation()` in `ChatAndApprovalsStores.swift` drop a pending live approval that isn't fetched again.
* There's no in-flight guard on send or on approval answers, so a double Return creates duplicate runs.
* The core's session route omits `tainted`, so a reopened tainted conversation doesn't show the "Outside content" chip (`src/server.rs:458-465`, `Wire.swift`). CONFIRMED in code: the JSON has no `tainted` key.
* "Try again" re-sends the last user message even after an approval or composer error.
* "Continue with version" does nothing until Settings has been opened once.
* On a core restart, `.starting` is never published, so calls go to the dead endpoint until `.restarted` (`CoreProcess.swift:274`). PLAUSIBLE.
* Events that fail to decode are dropped silently (`EventStream.swift:52`).

**Gates and tests** (finder-only):
* G4 accepts any containment JSON that has a valid `status` (`eval/containment/check.py`).
* The `verify-v1.sh --resume` staleness check omits `assets/`, `build.rs` and `docs/ACCEPTED_RISKS.md`.
* Several `pass_rules.py` checks are looser than their names say.
* Fixture tests pass when a fixture is missing (`tests/integration.rs:~1256`).
* `mode_survives_restart` depends on `HERON_MODE` in the environment.

**Cleanup and efficiency:**
* Every memory save re-reads every embedding and up to 3000 rows, synchronously (`src/memory.rs:268`).
* Each notification spawns an OS thread just to wait on the helper process, so a hung `notify_command` leaks a thread per notification (`src/app.rs:2260-2290`).
* `cancel` has two copy-pasted loops that clear approvals (`src/server.rs:~647-660`).
* `is_local` matches substrings (`src/server.rs:287`), so it accepts `localhost.example.com`. It only affects the "has key" display. Use `permissions::url_host_obviously_local`.

### Dropped or already tracked

* The claude-cli `is_error` result with exit code 1 is accepted as a reply (`src/cli_llm.rs:2004-2018`). Already tracked: Known low "limit phrases", and the claude-cli limit text that arrives in the stdout result. Fixing #70 or Improvement 7 should cover the auth half.
* `"always"` shell rules key on the first word. This is by design: `can_always` already refuses risky and tainted requests. Kept as an idea for Upgrade 2.
* Globs and quotes get past the secret check on Linux and Windows. This is the G3 M-4 family, closed for macOS; the Linux and Windows parts are outside the v1 Tier 1 platforms.
* The outside-read tier covers only the home folder on Linux and Windows. Same reason.

## Part 2: open fix PRs

### PR #70: CLI login probe and the experimental label

* **MEDIUM, CONFIRMED live: a logged-out Claude Code still shows as signed in.**
  * **Where:** `capture()` on the PR branch (`src/cli_llm.rs:866-871`).
  * **Mechanism.** On a non-zero exit, `capture()` returns stderr only. With an empty config dir, `claude auth status --text` exits 1, prints `Not logged in. Run claude auth login…` on **stdout**, and leaves stderr empty. The parser gets `""`, which gives `logged_in = None`. The app maps `None` to signed in (`Wire.swift:291`, `loggedIn ?? true`).
  * **Result.** This is the exact case the PR sets out to fix.
  * **Fix.** On a non-zero exit, parse stdout and stderr together. Add a fake-CLI test that exits 1 with the logged-out text on stdout.
* **LOW:** a probe timeout or unrecognised output also reads as signed in and "Recommended". Consider showing "unknown" in the UI instead.
* **LOW:** the negation list misses phrasings like "not currently logged in", and "unauthenticated" contains "authenticated".
* **LOW:** no test covers a non-zero exit with stdout text, or which stream claude uses.

### PR #71: native notifications

* **MEDIUM, PLAUSIBLE: notifications from before the first subscribe are lost.** A notification published before the app's live-only SSE subscribe, or during a resync gap, is lost (`AppModel.swift:165`, the `EventStream` resync). The core has already stopped its `osascript` fallback once the app is attached.
  * **Fix.** Fetch unread notifications on subscribe and after a resync, or keep the Inbox as the source and only notify from it.
* **MEDIUM, PLAUSIBLE: banners stop with no fallback.** After `serve --print-endpoint`, the core never uses `osascript`. The poster is `nil` outside a `.app` bundle (only `swift run`), and when permission is `.denied` it drops notifications. The PR comment says that is deliberate: the result still reaches the Inbox. Nothing tells the owner that banners are off.
  * **Fix.** Show "Notifications are off" in Settings with a button to open System Settings.
* **LOW:** two `requestAuthorization` calls at the same time while the status is `.notDetermined` can drop a banner (`try? … ?? false`).
* **LOW:** tainted job output, and a title of any length, appear verbatim in a banner that says "Heron", including on the lock screen.
  * **Fix.** Cap the length, and show "a job finished; open Heron" for tainted output.
* **LOW:** `lastNotifiedSeq` is set before the post, so a failed post is never retried. Events with seq 0 skip the de-duplication.
* **LOW:** no tests cover the authorization states or `Delivery::NativeApp` in the real `serve --print-endpoint` integration test.

## Part 3: improvements, most impact first

| # | What | Why it matters to the owner | Effort | Files |
|---|---|---|---|---|
| 1 | Close the three trust leaks: #1, #2, and #3 (memory undo and edit) | These are the owner's own invariants (taint, memory trust). Each is a small, local fix with a clear test | 1 day in total | `src/agent.rs`, `src/mcp.rs`, `src/server.rs`, `src/store.rs`, `ContentStores.swift` |
| 2 | Panic hardening: a run guard that always frees the conversation and sends `run_end` (#5); a `str::get`-based slice helper used everywhere model, user or email text is sliced (#4, #10, #16, the `[a..=b]` sites, `host_only`, `parse_trigger`); a clippy lint (`clippy::string_slice`) in CI | One bad character from a model shouldn't leave a conversation stuck as busy | 1 day | `src/server.rs`, `src/cli_llm.rs`, `src/app.rs`, `src/memory.rs`, `src/sources.rs`, `src/schedule.rs`, `.github/workflows/ci.yml` |
| 3 | Fix PR #70 before merging: parse stdout on a non-zero exit | Otherwise the PR ships the bug it was opened to fix | 1 h | `src/cli_llm.rs` (PR branch) |
| 4 | Run approvals separately from the HTTP request (#8) | Long approved commands (builds, tests) are the main thing Heron asks about, and they currently fail after 60 s | half a day | `src/server.rs`, `src/app.rs`, `ChatAndApprovalsStores.swift` |
| 5 | Shell and sandbox hardening: #6, #7 (owner decision), #13, #17; show the payload on approval cards (#14) | This narrows what Auto mode does silently | 1-2 days | `src/permissions.rs`, `src/sandbox.rs`, `src/browser.rs`, `src/tools/personal.rs` |
| 6 | Make CI stricter: require Seatbelt, run the eval tests, and gate `release.yml` on them (#11) | A green CI should mean the sandbox was actually tested | 2-3 h | `.github/workflows/ci.yml`, `.github/workflows/release.yml` |
| 7 | Have one Heron own triggers and proactive work (#9); make the audit chain safe across processes (#12) | No double job runs or false "tampered" alarms when the CLI and app run together | 2-3 h | `src/app.rs`, `src/store.rs` |
| 8 | Mac app polish pass: approval double-submit guard, fetch approvals again on `open()`, show the "Outside content" chip on reopened sessions, correct "Try again", load providers at bootstrap, publish `.starting` on restart | These are the daily rough edges an owner hits first | 1-2 days | `macos/HeronMac/Sources/HeronUI/Stores/*`, `CoreProcess.swift`, `src/server.rs:464` |
| 9 | Evaluator audit: run `genesis audit` on `genesis_verifier.py` and isolate the candidate (#15); tighten G4 and `pass_rules` | G7 and G4 are release gates. A gate that can be gamed proves nothing | 1 day | `eval/` |
| 10 | Docs drift: add this review's lows to "Known low issues". Set `tests/fixtures/api/providers.json` to `experimental: true` for opencode (already known). Record #7 in `docs/ACCEPTED_RISKS.md` if the owner accepts it | The spec is the source of truth | 1-2 h | `docs/designs/heron-v1.md`, `docs/ACCEPTED_RISKS.md`, `tests/fixtures/api/providers.json` |

## Part 4: upgrade plan

This part builds on the harness research of 2026-11-22: a synthesis of Harness² against Heron, about 25 local repositories and about 80 GitHub repositories. That research found six gaps in Heron's harness:

* no effect journal;
* no run status decided by a completion check;
* no loop detection beyond identical calls (such as A-B loops);
* compaction that loses detail for good;
* no protected paths;
* no harness versioning.

The upgrades below are ranked by value per unit of risk. Each one ports tested code from the owner's own repositories where it exists.

| Rank | Upgrade | Value | Risk | Effort | Owner decisions needed |
|---|---|---|---|---|---|
| 1 | **Effect journal and completion-decided run status** (from `anchor-harness`, `effects.rs`). Write "about to do X" before each tool call and "did X" after. A run is "success" only when a check the harness runs passes; otherwise it gets a precise failure status | Crash-safe resume, and an honest answer to "did it run?". It also fixes the class behind #8 and the "running forever" approvals | Medium: it touches `execute_call` and the store schema | 3-5 days (M-L) | After a crash, does an interrupted call ask again, or is it reported only? Which runs get a completion check (jobs and Shortcuts first)? |
| 2 | **Protected paths and an egress allowlist** (from `anchor-harness`). Files that run later (`.git/hooks`, `.envrc`, `.mcp.json`, toolchains) become read-only to the agent, or are hash-checked after each run. Optionally, sandboxed network traffic goes through a host allowlist | Addresses #7 and the general case of files that run later. It completes the sandbox story | Medium: it can break builds if the list is too tight | 2-3 days (M) | Which paths are protected. Whether toolchain writes stay allowed. Whether egress is limited by default or opt-in |
| 3 | **`RepeatGuard` loop detection** (from `anchor-harness`, `repeats.rs`, 204 lines). Catches A-B and A-B-C call cycles, and results that differ only in numbers | Fewer wasted steps and tokens, especially on CLI providers with usage limits | Low: it only adds a stop condition | Half a day to 1 day (S) | None (a default threshold) |
| 4 | **Recoverable context stack** (from `AXIOM-AETHER`). Large tool outputs become a short digest that can be expanded again, in place of Heron's lossy summary compaction, plus a cost ledger | Long tasks stop forgetting what they read. It also cuts tokens | Medium: digests of tainted output must stay tainted and wrapped | 3-4 days (M) | How long tool outputs stay on disk, and whether they're encrypted. Digests never enter memory without the trust rules |
| 5 | **MCP tool pinning, per-argument policy and a tamper-evident audit** (from `Airlock`). Pin each MCP tool's definition by hash and ask again when it changes. Add rules on individual arguments | Stops an MCP server from quietly changing its tools after approval. It builds on #2 and #12 | Low to medium | 2-3 days (M) | Re-approve on any change, or only on changes to the schema and description? |
| 6 | **A fenced harness² improvement loop** (later). An offline optimiser may edit only the "How to work" text, tool guidance and skills. It runs on a suite of 50 or more tasks with held-out scoring for each CLI, and adopts a change only through a PR that passes a baseline comparison. It never touches the Safety block, permissions, taint, sandbox, containment, memory trust or the eval thresholds | Steady gains without changing the model | High while the eval set is small: G7 has 11 tasks and would overfit | 2+ weeks (L), after upgrades 1-3 | Owner approval of the file allowlist; lessons never reach the prompt without approval; a spending budget for each CLI |

**Also worth checking first.** The research found that Claude CLI hooks *fail open*, while its permission-prompt tool *fails closed*. That bears directly on `src/cli_llm.rs` containment. It's unconfirmed on the current CLI version, so re-test it before relying on hooks.

**Mac app beyond fixes.** The F1-F9 feature build is already running on `feat/v1-*`, so this plan doesn't propose separate Mac upgrades. After it lands, the polish pass (Improvement 8) is the highest-value Mac work.

## What this review could not do

* **Ultra wasn't available** for this account. Run `claude auth login` to refresh the entitlements, then re-run `/code-review ultra 62` if a cloud review is still wanted. This review is the local **max** fallback.
* **Finder-only items** in "Other candidates" weren't checked by hand. Treat them as leads.
* **No code was run** except two harmless checks: replaying the shell regexes in Python, and running `claude auth status` with an empty config dir. No tests, mutation runs or Seatbelt experiments were run.
* **Worktree mix-up, now corrected.** Partway through, finder agents checked out the PR branches in the shared worktree. The worktree was put back on `main`. Every ranked finding's lines were checked again on `main`, as were the finder-only cites in files that #70 or #71 touch. Other finder-only line numbers are approximate (marked `~`).
