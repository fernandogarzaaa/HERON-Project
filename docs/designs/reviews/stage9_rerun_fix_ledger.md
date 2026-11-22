# Stage 9 re-audit fix ledger

Source: `implementation_readiness_rerun_report.md` (3 Blockers, 30 Majors, 22 Nits, at d9c28c4). The owner chose "fix them, no re-run", so this ledger is the record. No further re-audit was run. Each row gives the fix and where it lands in `heron-v1.md`. "Item N" is the Detailed Implementation Plan item.

Code facts checked for this pass (2026-11-18):
- Commit `9253959` exists locally: "Prepare HERON v0.3.1 macOS packaging fix release (#23)". At that commit `heron run` takes `-C <workspace>`, and the demo model calls a tool for "save", "task" and "list" prompts.
- `usage` events carry running totals (`agent.rs:376-381`); the JSON keys are `prompt` and `completion` (`server.rs:431`).
- `/api/chat` spawns the turn before returning its SSE response, and uses `409` for a busy session (`server.rs:442-518`).
- `run_now` and `run_triggers` call `run_job`, which returns `bool`; `JobHost::ask`, `reflect_if_due` and `proactive_tick` call `llm.chat` directly (`app.rs`).
- `ci.yml` uses a shallow checkout and `rustup update stable`; `release.yml` references no secrets, and its title uses `$RELEASE_TAG`.
- `test_cfg` sets `cfg.home` to the folder `fake_bin` writes into (`cli_llm.rs:2008-2030`); 22 tests call `fake_bin`.
- The model trait is `Llm`, wrapped by `SwappableLlm` (which holds a `Store`) and `Cassette` (`llm.rs`).
- `ApprovalRequest` has no rule field (`permissions.rs:288-299`); `ToolCtx` carries `session_id` and `store`.
- `sandbox::available()` and `sandbox::active(cfg)` both exist (`sandbox.rs:46, 52`).
- `cargo clippy --locked --all-targets --keep-going -- -D warnings` reports errors only under `src/`, but that doesn't prove `tests/` is clean (see B2).

| ID | Fix, or reason for dismissal | Lands in |
|---|---|---|
| B1 | `upgrade-test.sh` runs the bundled `dist/Heron.app/Contents/MacOS/heron-cli` (overridable with `HERON_NEW_BIN`), and exits 2 when it's missing. `make-upgrade-fixture.sh` copies the 0.3.1 binary to `$OLD_BIN` outside the worktree before removing it. A `--binary-only` mode rebuilds just that binary, and `upgrade-test.sh` calls it when the binary is missing. CI uses `fetch-depth: 0`. U7's check also tests that `heron-cli` exists. | Items 45, 46, 47; table U7 |
| B2 | **Confirmed, then fixed (corrected after the merges).** The first dismissal was wrong: under `-D warnings` a failing lib stops cargo from linting the crates that depend on it, so `--keep-going` couldn't see `tests/`. Run as warnings, `tests/integration.rs` has 9. They were fixed on `feat/heron-v1` before `skein run`, with no behaviour change, and the 10 integration tests still pass. | Item 13 |
| B3 | `chat` checks the risk notice before it creates a session, and returns `428`. The busy `409` keeps its status and gains `{"error":"session_busy"}`. Errors after the stream starts, `CliVersionUntested` included, become SSE `error` events with a `code`. New U3 tests: `chat_returns_428_before_the_notice_is_accepted`, `busy_session_returns_409_with_its_code`, `untested_cli_version_arrives_as_an_sse_error_code`. The spec's API contract and risk-notice text match. | Items 35, 37, 38; spec §3, §6; table U3 |
| M1 | Each entry script inserts its own folder into `sys.path`. `test_runner.py` checks the import under `-I`. | Items 4, 9 |
| M2 | The adapter gains `--record <file>`. Step 9 spells out the `eve` driver: start the adapter, set `HERON_ADAPTER_PORT` and `HERON_TASK_PROMPT`, run `eve chat` with the config, stop the adapter, and read the record. The `eve chat` argv stays UNVERIFIED until U0 reads its help. | Items 3, 4 |
| M3 | `tokens` comes from the last `usage` event. The medians cover gate-task runs with no error. `run-spike.sh` passes `--tasks t01,…,t10`, giving 50 runs per arm, and `--spike` checks that. | Items 3, 4, 5, 32 |
| M4 | `{web}` is replaced in every copied file, `work/` files included, and a runner test covers it. | Items 1, 2, 4 |
| M5 | The CLI profile gets its own write list (the run folder, `/dev`, its `config_dirs`) and its full rule order. It reads the interpreter folder of a script binary, and the `current_exe()` folder only in the hybrid arm. It gets no `.gitconfig` and no shared temp folders. Spec §2 matches. | Item 26; spec §2 |
| M6 | `SEATBELT_SECRET_PATTERNS` leaves out the `/proc`, `/sys`, `/dev` and `/etc` alternatives. `build_and_git_still_work_under_deny_default_reads` runs under it. | Item 26 |
| M7 | The fake-CLI tests use sibling `home`, `bin` and `out` folders. All 22 are listed, and `cancelled_chat_removes_its_temp_dir` is updated. | Items 25, 30 |
| M8 | `CliLlm` holds `cfg: Arc<Config>` and `store: Option<Store>`. The `POST /api/settings` body shapes and the Mac Settings controls are given. "No version match" is treated like out of range. The logged-out message uses each entry's existing `login_command`, and the login texts stay UNVERIFIED until U2c. | Items 24, 25 |
| M9 | The trait is `Llm`, with a default identity. `SwappableLlm` and `Cassette` forward it. `AuditEntry`, `audit_log()` and the activity route return `provider` and `cli_version`, and the `audit` fixture is added. The diagram label is fixed. | Items 25, 28, 38; spec §0 |
| M10 | `ApprovalRequest` gains `always_rule: Option<String>`, computed once in `execute_call` and used by every approver. `Always` with `None` means once. A too-broad conversation grant stores an exact `read=:` entry. Every `always_rule` caller is listed. | Items 14, 17, 35 |
| M11 | Product call. Default: the shell sandbox also reads the conversation's `read:` and `read=:` grants. Asked as **Q20**, and §1 says so. | Q20; spec §1; item 26 |
| M12 | `observe`, `recent_changes` and `scan_folder` take `Arc<Config>` or `&Config`, and an `Arc` is moved into each `spawn_blocking` closure. The callers and tests are listed. | Item 21 |
| M13 | `run_job` returns a `RunStatus` enum. `run_due_schedules` and `run_triggers` pass `attended = false`, and `run_now` passes `true`. The hold is checked in `run_job`, `JobHost::ask` and `JobHost::agent`, so `workflow.rs` doesn't change. A held step stops the workflow. The rule reads "only when all four hold". | Item 29 |
| M14 | `SwappableLlm::chat` refuses before the notice is accepted, which covers every model call that bypasses `run_turn`. New test `unattended_model_calls_wait_for_the_risk_notice`. | Items 33, 37; spec §6 |
| M15 | Listed with new expectations: the two `server.rs` stream tests (deleted or rewritten), `proactive_levels_suggest_then_take_over_within_bounds` (parks), `the_agent_cannot_rewrite_its_own_settings_unasked` (deny), and `every_built_in_tool_declares_what_it_changes` (two rows). U6 rewrites `SECURITY_THREAT_MODEL.md:68`. | Items 23, 37, 42 |
| M16 | Spawners publish `done` or `error`, then `learned`, then the new `run_end`, and `/api/chat` ends on `run_end`. A `since` past the last `seq` gets `resync`. `event_json` moves to `events.rs`. The `approval_resolved.decision` values and the `/api/grants` JSON are given. `JobHost::agent` uses `sink_for`. `events-error.jsonl` comes from a `ScriptedLlm` failure. | Items 34, 35, 38; spec §3 |
| M17 | `server::endpoint_line(&Started)` is tested in the lib. `require-tests.sh` matches the last `::` component exactly. | Items 6, 35, 36 |
| M18 | U2c's check only validates the records (`check.py --recorded`). The G4 gate runs in U2w's check and in `verify`. A failure caused in `src/` adds a new `src/**` node. | Item 31; table U2c, U2w; Order |
| M19 | Tier 1 needs a passed check, confirmed folders and the gemini/antigravity rule. Every entry outside Tier 1 is set `experimental = true`, and the test asserts that. Folders are found with an owner-run fresh-`HOME` login (`discover-dirs.sh`, written by U0). | Items 8b, 31, 39; Preconditions |
| M20 | Exact headings and anchors for C35-8 and R-1. U3a adds `## Accepted risks` sections to both engineering files in their existing formats. The G2 notice goes to the README and the guide, not the register. U3a's check greps the anchors. | Items 11, 12; table U3a |
| M21 | The backend allowlist covers each unit's own check commands. Completion commands run in skein, outside the agent's list, and the backend carries the same code-execution caveat as skein's default. | Preconditions |
| M22 | The six new secrets are named as an owner precondition, and the build falls back to `--unsigned` without them. The title uses `$RELEASE_TAG`. `fetch-depth: 0` is set. The commit id is confirmed. | Preconditions; item 47 |
| M23 | The owner commits `rust-toolchain.toml` before `skein run`, and `ci.yml` drops `rustup update stable`. | Preconditions; item 47 |
| M24 | `macos/HeronMac/.gitignore` holds `.build/`. The open UI choices go to the implementer, recorded in `DESIGN-CHOICES.md` and reviewed in the second PR. The first run shows the sandbox status. U4's check tests both files. | Item 40; table U4 |
| M25 | Copying the new tests onto `BASE` would only fail to compile. Instead the G3 reviewer records each regression's old-code failure in `docs/designs/reviews/g3-regressions.md`, and `verify` step 5 requires a line for each of the seven `m*` tests. | Item 48 |
| M26 | The test renders the prompt with every source filled in, checks that every header is registered, and checks that the removed blocks are absent. | Item 23 |
| M27 | `Profile::Cli` is Seatbelt only. On Linux and Windows the CLI child runs unwrapped, as today. | Items 25, 26 |
| M28 | `permissions::check_workspace`, with `WorkspaceRefused`, is called first in `run_turn`, so every surface gets it. | Items 14, 36 |
| M29 | `owner-use.sh --min <k>` exits 1 below `k` and requires a 14-day range. The `verify` G9 step fails when it's unmeasured, unless `HERON_G9_PENDING=1` is set. | Items 7, 48 |
| M30 | The sentence is split. Tool-wide rules stay in the matcher for other effects, and both kinds are listed in `rule_notices()`. | Item 14 |
| N1 | U6b greps one exact new sentence. U6 greps exact sentences and the threat-model change. | Items 42, 43; table U6, U6b |
| N2 | Changed to `matches!(effect, Refused \| SensitiveRead)`. | Item 21 |
| N3 | The `add_allow` callers and how each handles the `Result` are listed. | Item 14 |
| N4 | `browser.rs` `FnTool` literals are listed, and the heading is updated. | Item 15 |
| N5 | Cited at `fs.rs:1235-1238`; the `.ssh` symlink test stays `SensitiveRead`. | Item 23 |
| N6 | "Every test, through a helper or inline", with the inline sites listed. | Item 37 |
| N7 | The Windows claim is fixed, the sandbox-off tests are regrouped, and the call is named `sandbox::active(&cfg)`. | Item 30 |
| N8 | `-C <tmp>/work` and three prompts that make demo tool calls. | Item 45 |
| N9 | The two variables are added to the `ReadRoots` cache key. | Item 14 |
| N10 | One type, `Option<&Path>`, in the spec and the plan. | Spec §1 |
| N11 | Exit codes 0 to 3, each with one meaning. | Items 3, 4 |
| N12 | Probe ids `p1`, `p2`, …; t02 uses two `file_contains` rules; t07's JSON escaping is shown; "(shortened)" becomes "(exact text)". | Item 1 |
| N13 | Default mode requires the provider, driver, mode, build and runs; `--spike` requires `build = "debug"`. | Item 5 |
| N14 | The baseline commit is defined as U0b's `heron_commit`, the start of `feat/heron-v1`, and `verify` names the field. | Definitions; items 10, 48 |
| N15 | `asks` comes from `audit` rows, as in the spec. | Items 3, 4 |
| N16 | The question-to-unit mapping is completed, and Q5 goes to U6's release checklist. | Preconditions; item 42 |
| N17 | Both places count `## R-` headings, and C35-8 isn't one. | Items 11, 48 |
| N18 | The U1 heading and the item headings match their content. Every citation and the clippy count are re-checked after the merges. | Preconditions; items 15, 21, 24, 25, 29 |
| N19 | `[0-9]` instead of `\d`, and a bad date exits 2. | Item 7 |
| N20 | "The parent's `$TMPDIR`" is spelled out, and the `409` overload is fixed under B3. | Item 25 |
| N21 | U2w drops the flag if the canary passes with tools off. | Item 25 |
| N22 | U2s depends on U2c, so they don't share a login at the same time. testteeth's scope is stated. | Table U2s; Order; item 48 |
| Prior N12 | **Still open.** PRs #26 and #35 aren't merged. The precondition stands, and now covers every citation and the clippy count. | Preconditions |

**Still untested before approval:** Q19's skein blast-radius behaviour for `README.md`. The test is written up in Q19.
