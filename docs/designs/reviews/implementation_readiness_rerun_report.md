# Goldfish Review Report (re-audit)

**Target:** `docs/designs/heron-v1.md` (`Lines 744-1694`, "## Detailed Implementation Plan" to end of file, at commit d9c28c4)

**Stage:** 9, implementation-readiness test, re-audit run (Mode: REVIEW)

**Prior report:** `docs/designs/reviews/implementation_readiness_report.md` (48 findings at ff5a31d)

**Reviewers:**
- `egm-implementation-readiness` and `egm-codebase-details`, both against `implementation-readiness-standard.md`.
- `egm-style`, against its persona's default.
- The Work units rule audit was done by the synthesizer (lens `synthesizer`).

**Verdict: not ready, but much closer.**
- Of the 48 prior findings, 35 are resolved, 12 are partly resolved and 1 is open. All 8 prior Blockers are resolved or partly resolved.
- The Work units table passes on columns and dependency order. It also passes on scope form, apart from the one declared exception, which is justified but not yet proven to work with skein.
- Three units still can't pass their own check as written:
  - U7: the upgrade test runs two binaries that don't exist when it runs (B1).
  - U1, and with it U2 and U3: the clippy gate probably fails in `tests/integration.rs`, which those units can't edit (B2).
  - Ut: it asserts an HTTP status that the planned `/api/chat` can't return (B3).

**Counts after deduplication (new findings):** 3 Blockers, 30 Majors, 22 Nits.

**How the findings were checked:** I checked the repository facts behind each Blocker myself.

| Blocker fact | Where |
|---|---|
| `build-app.sh` builds only with `--target <triple>`, so nothing writes `target/release/heron` | `packaging/macos/build-app.sh:7-10`; plan line 1566 keeps that form |
| The plan removes the 0.3.1 worktree, then still expects the binary inside it | plan lines 1586, 1588, 1598 |
| `tests/integration.rs` uses `let mut cfg = Config::default(); cfg.x = …` (the `field_reassign_with_default` pattern) | `tests/integration.rs:27-28, 191-192, 348-349, 394-395, 499-500, 572-573` |
| No lint is configured anywhere that could suppress it: no `clippy.toml`, no `rust-toolchain` file, no crate-level `#![allow(clippy…)]` and no `[lints]` table | repo root, `tests/*.rs`, `src/lib.rs`, `src/main.rs`, `Cargo.toml` |
| `/api/chat` creates the session, spawns the turn, then returns the SSE response, so `run_turn`'s error can't change the HTTP status | `src/server.rs:442-518` (spawn at 469, `run_turn` at 493) |
| `/api/chat` already uses `409` for a busy session | `src/server.rs:455-457` |

I also checked these facts behind Major and Nit findings:

| Fact | Where |
|---|---|
| `usage` events carry the running App-wide total, not a per-step delta | `src/agent.rs:376-381` |
| The secret pattern includes `^/(proc\|sys\|dev)/` and `^/etc/(shadow\|sudoers\|passwd)` | `src/permissions.rs:541` |
| `release.yml` references no secrets at all, and the title uses `$RELEASE_TAG` | `.github/workflows/release.yml:46-164` |
| `README.md` already contains "outside the working folder" | `README.md:135` |
| `.gitignore` has only `target/` and `dist/` | `.gitignore` |
| No `C35-8` entry exists anywhere outside the design docs | grep across the repo |
| `uuid` (v4), `regex` and `tokio` "full" are already dependencies, so U2 and U3 need no `Cargo.toml` edit | `Cargo.toml:35, 44, 48` |

The rest of the Major and Nit evidence comes from the reviewers' reads, and I did not re-check it line by line. Where two reviewers reported the same fact on their own, I merged their findings and kept the evidence they agreed on.

**Precondition still not met:** PRs #26 and #35 are not in this tree. No `C35-8` entry exists in `docs/engineering/`, which U3a's check relies on (M20).

---

## Work units rule audit (lines 1640-1669)

| Rule | Result |
|---|---|
| id, what to build, depends on, check and model role on every row | **Pass.** All 16 rows have them. Every model role is a `pstack-models.md` line (`feature, refactoring`, `hardest tasks`, `judgment and prose`). The unit headings match the table rows one for one, and every test named in a check is listed in its unit's Tests item. |
| Scope is one directory glob | **Pass, with one declared exception that is justified in principle but not yet proven to work with skein.**<br>- Every row except U6b has one directory glob. `verify` now owns `scripts/**`, and the earlier `Cargo.toml` and `tests/` needs are gone: no new crate is needed (`uuid`, `regex` and `tokio` are already dependencies), and U1 to U3 keep their tests inline.<br>- **Q19 (U6b, `README.md`; spec lines 181-182).** The reasoning holds. `README.md` has to stay at the repository root, and the only directory glob that covers it is the whole repository, which would conflict with every other node. A single-file scope is the narrowest one that says what U6b writes.<br>- **The risk.** skein's documented matcher compares the *static directory prefix* of each blast radius. For a root file that prefix may be the repository root, and then U6b would conflict with every node running at the same time. In practice that would only serialize U6b against other nodes, and when U6b runs only `verify` is still pending. But nobody has checked how skein treats the scope.<br>- **What to do before approval.** Run `skein node add` with `--blast-radius README.md` next to an active `src/**` node and see whether skein reports a conflict. Record the result in Q19. Separately, U6b's check is partly vacuous (N1). |
| Scope covers every file the unit writes | **Pass, with two caveats.**<br>- U1, U2 and U3 run `cargo clippy --all-targets`, which lints `tests/integration.rs`, and they can't fix lints there (B2).<br>- U4's `swift build` and U7's `build-app.sh` write `macos/HeronMac/.build/`. No `.gitignore` covers that folder, so skein may commit build output, and in U7's case it lies outside U7's scope (M24). |
| Check is a shell command that exits 0 | **Pass on form.** Every check is a runnable command, and `eval/require-tests.sh` stops a renamed test from silently dropping out of a check. Caveats:<br>- U3's check uses `--lib` for a test that may live in the `heron` bin target (M17).<br>- U6b's third grep already passes on today's README (N1).<br>- `verify` step 9 fails until the owner signs R-1. The plan says that is intended. |
| Check can pass with only that unit's files and its dependencies built | **Fail** for 3 units, plus 2 conditional cases:<br>- **U7.** `upgrade-test.sh` runs `target/release/heron`, which the check never builds. The 0.3.1 binary it needs for the rollback is deleted with its worktree (B1).<br>- **U1** (and U2, U3). Clippy `-D warnings` probably fails in `tests/integration.rs` (B2).<br>- **Ut.** `risk_notice_blocks_chat_until_accepted` expects `428`, which U3's design can't return, and Ut can't edit `src/` (B3).<br>- **U0, U0b, U2s and `verify` step 6, conditionally.** Under `python3 -I`, `run_core_tasks.py` can't import `pass_rules` or `eve_adapter` (M1). The unit can fix this inside its own scope, but the plan doesn't say how.<br>- **U2c, conditionally.** Its check needs claude-cli and one other CLI to pass. A failure with a cause in `src/` can't be repaired in `eval/containment/**`, and U2w waits on U2c (M18). |
| Dependency order is buildable | **Pass.**<br>- The graph is acyclic.<br>- The four `src/**` units run in sequence, U1 → U2 → U3 → U2w, with Ut (`tests/**`) between U3 and U2w.<br>- The two `docs/**` units are ordered (U6 depends on U3a).<br>- The `eval/**` units that overlap (U0 with U0b, U2c and U2s) are ordered by their dependencies. U2c and U2s, which run at the same time, have disjoint scopes.<br>- `include_str!("../docs/ACCEPTED_RISKS.md")` in U3 resolves, because U3 depends on U3a.<br>- `verify` lists its 15 dependencies by id.<br>- One cost to note: U2c and U2s run at the same time on the same claude-cli login (N22). |

---

## Prior findings: status

| Prior | Title (short) | Status | Note |
|---|---|---|---|
| B1 | U1 uses work only U2 delivers | Resolved | `config_dirs` is now in U1 (item 22); `sessions.origin` is in U2; the M-4 test is split into a `read_file` half and a sandbox half. |
| B2 | Risk notice embeds a file U6 creates later | Resolved | The new U3a creates the file before U3. The `include_str!` path and the `^Version: (\d+)$` rule are given. New gap: the threat-model entries it links to don't exist (M20). |
| B3 | Sandbox-off refusal breaks tests and non-macOS CI | Partly resolved | `required()` is macOS only, and the shell tests are listed. Still missing: the fake-CLI tests under the CLI profile (M7) and the CLI's wrapping on Linux (M27). |
| B4 | Folder grants can't match allow rules | Resolved | `Call.read_target`, `read_rule_covers`, only `read:` rules grant OutsideRead, the read-tool "allow" switch is refused, and the promises test is extended. The approver plumbing is new (M10). |
| B5 | G7 gate and runner contract undefined | Resolved | All 11 tasks, the pass-rule grammar, `summary.json` schema 1 and the exit codes are given. Residual issues: M2, M3, M4. |
| B6 | U0 runner can't drive Heron | Partly resolved | The token parse, the `/api` prefix, `base_url = "demo"` and the adapter are specified. Still missing: in `eve` mode, how the turn data gets back to the runner, and the `eve chat` invocation (M2). |
| B7 | U5 empty but verify depends on it | Resolved | U5 is removed, and `verify` lists its dependencies. |
| B8 | Scopes don't cover files written | Resolved | Tests are inline, there are no `Cargo.toml` changes, the runner has `--topology`, `verify` owns `scripts/**`, and U6b is the declared exception. New: clippy over `tests/` (B2). |
| M1 | Refused representation and callers | Partly resolved | `Effect::Refused`, `classify_path(…, cfg, write)`, `ReadRoots` and the deletion of `read_effect` are given. But `&app.cfg()` can't cross the `spawn_blocking` closures in ambient and sources (M12), and `CliLlm` has no `Config` (M8). |
| M2 | OutsideRead underdefined; check order | Resolved | The property table, the check order and the step-9 arms are given. |
| M3 | Grant scope and broad-grant rules | Partly resolved | `too_broad`, `add_allow -> Result`, `rule_notices` and no write-back are given. `ApprovalRequest` carries no target, and the terminal and Telegram approvers are omitted (M10). |
| M4 | Walkers don't filter Refused | Resolved | |
| M5 | MCP taint refuses every write | Resolved | MCP is read-only for state writes; `server_speaks_mcp` and the integration test are updated. |
| M6 | `recall` partial taint, `recent_activity` | Resolved | `recall_unconfirmed` is split out, and `recent_activity` is defined. |
| M7 | Session origins not mapped | Resolved | A call-site table and `create_with_origin` are given. |
| M8 | Audit v2 hash and field plumbing | Resolved | The v2 formula is length-prefixed, and `audit_row` is given. The "Provider trait" it relies on doesn't exist (M9). |
| M9 | Duplicate pre-migration backup | Resolved | The existing snapshot is reused. |
| M10 | Config and Cargo changes missing | Partly resolved | `ApiKeyAllow` is defined, and no Cargo change is needed. But `set_settings` assigns a bool and `CliLlm` stores a bool, and neither is addressed (M8). |
| M11 | Subscription hold not implemented | Partly resolved | `model_held`, the statuses, the tests and the accept-version route are given. The hold is wired through the wrong call path (M13). |
| M12 | Per-provider registry data missing | Resolved | The `config_dirs` table, the API-key variables, the Cline decision and the version-manager order are given. New gaps on the Tier 1 rule: M19. |
| M13 | Sandbox API and profile undefined | Partly resolved | `Profile`, `wrap`, `required()`, the skeleton and the hint are given. The CLI write set contradicts §2 (M5), and the secret-pattern rules would deny `/dev` (M6). |
| M14 | `mcp-serve --refuse-all` not buildable | Resolved | |
| M15 | Event bus can't see background runs | Partly resolved | The bus is in `App`, and its types, events and the cancel response are given. Still undefined: who publishes `done`, `error` and `learned`, when a stream ends, some event and route shapes, and the `JobHost::agent` sink (M16). |
| M16 | Risk-notice gate breaks surfaces | Partly resolved | Platform scope, `accept_current`, the runner's accept step and the terminal flow are given. New problems: `428` can't be returned (B3), and model calls that bypass `run_turn` are not blocked (M14). |
| M17 | `--print-endpoint` and missing routes | Resolved | `start_endpoint`, the providers, grants and accept-version routes, and `background_owner` are given. The test's placement is new (M17). |
| M18 | G9 count undefined | Resolved | The exact SQL and `owner-use.md` are given. New: the verify step can't fail (M29). |
| M19 | Verification gates mis-specified | Resolved | The base is the baseline commit, CI gets clippy, the trial dates come from environment variables, the genesis command is given, and fmt is explicitly out. New: the toolchain is unpinned (M23). |
| M20 | Xcode project creation open | Resolved | Replaced by a Swift package. New UI gaps: M24. |
| M21 | `build-app.sh` and signing incomplete | Resolved | The steps are spelled out. The "existing secrets" the plan cites don't exist (M22). |
| M22 | Upgrade fixture and release label | Partly resolved | The fixture script, the upgrade test and the label are given. But the binaries it runs are missing (B1). |
| M23 | U6 and U6b targets and headings | Resolved | |
| M24 | Checks can pass running zero tests | Resolved | `require-tests.sh`. Residual: the `--lib` placement (M17). |
| M25 | Spike can't select hybrid | Resolved | |
| M26 | Secret pattern not exact | Resolved | The exact alternatives are given. |
| M27 | Breaking tests not listed | Partly resolved | Many are listed now, but several server, app, agent and fake-CLI tests are still missing (M7, M15). |
| M28 | Telegram forward fix underspecified | Resolved | It is re-checked after #35. |
| N1 | Commands written differently | Resolved | |
| N2 | Rules stated twice | Resolved | |
| N3 | Absolute guarantee | Resolved | |
| N4 | Unlinked diagram, informal phrase | Resolved | |
| N5 | "Only for loopback" ambiguous | Resolved | There is no entitlements file now. |
| N6 | U5 dependency range | Resolved | |
| N7 | Scheduling text unclear | Resolved | |
| N8 | CLI run-folder name | Resolved | `cancelled_chat_removes_its_temp_dir` is still unlisted (M7). |
| N9 | EVE config and baseline | Partly resolved | The baseline parameters are given. The EVE fields and flags remain UNVERIFIED (see M2). |
| N10 | Hybrid Claude flags | Resolved | They are given and marked UNVERIFIED. |
| N11 | Fragile checks | Resolved | |
| N12 | Preconditions unmet, line drift | Open | #26 and #35 are still not merged. Only the `agent.rs`, `telegram.rs` and `app.rs` citations are re-checked (N18). |

**Totals:** 35 Resolved, 12 Partly resolved, 1 Open.

---

## New findings (Blockers -> Major -> Nits)

### B1. U7's upgrade test runs two binaries that don't exist when it runs - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `style`, `synthesizer`
* **Affected Scope:** `Lines 1566, 1580-1588, 1594-1598`; table `1657`; `1620`, `1635`
* **Discrepancy Description**:
  - **Current binary.** Step 3 runs `target/release/heron audit`. `build-app.sh` builds only with `--target <triple>` (plan 1566, `packaging/macos/build-app.sh:8-9`), which writes to `target/<triple>/release/`. In a fresh skein worktree or CI runner, nothing in U7's check or the `macos-app` job builds `target/release/heron`. `verify` step 8 passes only because step 6's runner happened to run `cargo build --locked --release` first.
  - **0.3.1 binary.** Line 1588 keeps it at `$TMPDIR/heron-0.3.1/target/release/heron`, which is inside the worktree that step 6 removes (1586).
  - **Rebuilding it.** Line 1598 says the binary is "built by `make-upgrade-fixture.sh` when missing". But `--if-missing` "does nothing when the archive exists" (1580), and the archive is committed, so the binary is never rebuilt.
* **Rationale & Recommendation**:
  - Have `upgrade-test.sh` run the bundled `dist/Heron.app/Contents/MacOS/heron-cli`, which is the binary that ships, or build `cargo build --locked --release` first.
  - Have `make-upgrade-fixture.sh` copy the 0.3.1 binary out of the worktree (for example to `$TMPDIR/heron-0.3.1-bin/heron`) before removing it.
  - Add a `--binary-only` mode that rebuilds only that binary, and have `upgrade-test.sh` call it when the binary is missing.

### B2. U1's clippy gate probably fails in `tests/integration.rs`, which U1 can't edit - Severity: Blocker (probable)
* **Reviewer Lens:** `codebase-details`, `synthesizer`
* **Affected Scope:** `Line 969`; table `1647, 1648, 1651`
* **Discrepancy Description**:
  - Item 13 counts "57 errors, all in `src/`", most of them `field_reassign_with_default`.
  - `tests/integration.rs` uses the same pattern six times: `27-28`, `191-192`, `348-349`, `394-395`, `499-500` and `572-573`. `Config`'s fields are all `pub`, and nothing suppresses the lint: there is no `clippy.toml`, no crate-level allow and no `[lints]` table.
  - When the lib target fails under `-D warnings`, cargo never compiles the integration-test crate, which depends on the lib. So those hits would only show up after `src/` is clean, and that is consistent with the "all in `src/`" count.
  - U1, U2 and U3 all run `cargo clippy --locked --all-targets -- -D warnings` with scope `src/**`. They can't fix `tests/`, and Ut, the only `tests/**` unit, comes after them.
* **Rationale & Recommendation**:
  - Settle it with `cargo clippy --locked --all-targets --keep-going -- -D warnings`, and look for paths under `tests/`.
  - If there are hits, add a small `tests/**` lint-fix unit ahead of U1. Another option is to run `--lib --bins` clippy in U1 to U3 and leave `--all-targets` to Ut and later units.

### B3. Ut asserts a `428` from `/api/chat` that the planned handler can't return - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `synthesizer`
* **Affected Scope:** `Lines 1383-1384, 1406, 1425, 1463`; table `1652`
* **Discrepancy Description**:
  - Item 36 has `Agent::run_turn` return `RiskNoticeNotAccepted`. Item 35 spawns the turn as its own task, returns an SSE stream, and "maps" that error to `428`.
  - The handler sends its response headers before `run_turn` runs (`src/server.rs:469-517`). So the error can only arrive as an SSE `error` event under status 200.
  - `CliVersionUntested` is raised inside the model call and has the same problem. Its `409` also collides with the existing busy-session `409` (`server.rs:455-457`).
  - U3's named tests don't pin the status code. Ut (`tests/**`) asserts it, and Ut can't change `src/`.
* **Rationale & Recommendation**:
  - In item 35, have `chat` check `risk_notice::required() && !accepted(store)` before it creates the session, and return `428` from there. Keep the `run_turn` check for the other surfaces.
  - For `CliVersionUntested`, either define an SSE error event with a `code` field, or run a pre-flight check against the cached version with a distinct status, and give the busy `409` an `{"error":"session_busy"}` body.
  - Add a U3 lib test that asserts the status.

---

### M1. `python3 -I` drops the script's folder from `sys.path` - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 829, 851, 853`; table `1644, 1645, 1650`; `1633`
* **Discrepancy Description**: In isolated mode, `sys.path` contains neither the script's folder nor the user's site-packages (CPython docs for `-I`). So `run_core_tasks.py` can't `import pass_rules` or `import eve_adapter`, and the U0, U0b, U2s and `verify` checks fail. `unittest discover -t eval` isn't affected, because discovery adds the top-level folder itself.
* **Rationale & Recommendation**: Specify that each entry script starts with `sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))`, or merge the modules into one file.

### M2. In `eve` mode, the turn data has no way back to the runner, and the `eve` invocation is unspecified - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `style`
* **Affected Scope:** `Lines 809-823, 851`
* **Discrepancy Description**:
  - The pass rules need the events, the session id and the audit rows: `transcript_lacks` uses the events, and `audit_has` uses the audit rows of the run's session. `summary.json` needs `ttft_ms`, `tokens` and `asks`.
  - In server mode, the adapter returns only what EVE reads at `reply`.
  - The plan doesn't say who starts and stops the adapter, how `HERON_ADAPTER_PORT` and `HERON_TASK_PROMPT` reach EVE, or which `eve chat` command runs.
* **Rationale & Recommendation**: Give the adapter a `--record <file>` option that writes the full `turn()` result per turn. Spell out step 9 for the `eve` driver: start the adapter, set the variables, run the exact `eve chat` command, stop the adapter, and read the record.

### M3. Token totals are summed from running totals, and the spike's token metric is undefined - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 817, 863, 866, 891, 1347`
* **Discrepancy Description**:
  - Each `usage` event carries the App's cumulative total (`src/agent.rs:376-381`). Summing the events, as line 817 says, counts tokens many times over and distorts the spike's "no more than 5% more tokens" rule.
  - `median_tokens_per_task` doesn't say whether `p1` and errored runs count.
  - The spike runs every task, including `p1`, which gives 55 runs per arm, not the spec's 50.
* **Rationale & Recommendation**:
  - Per run, take prompt plus completion from the last `usage` event. Each run starts a fresh core, so that is the run's total.
  - Define the median as taken over gate-task runs that have no `error`.
  - Pass `--tasks t01,…,t10` in `run-spike.sh`.

### M4. `{web}` isn't replaced in working-folder files, so the probe can pass trivially - Severity: Major
* **Reviewer Lens:** `style`
* **Affected Scope:** `Lines 768, 795, 802`
* **Discrepancy Description**: `{web}` is replaced in the prompt and in the web files. `p1`'s `README.md` is a `work/` file that contains `{web}/collect`. Left as written, the injected URL is never real, so `no_request /collect` holds whatever the model does.
* **Rationale & Recommendation**: Replace `{web}` in every file copied from a task's `work/` tree too.

### M5. The CLI sandbox profile's read and write sets don't match the spec - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `style`
* **Affected Scope:** `Lines 1209-1223, 1194, 1305`; spec `385-387`
* **Discrepancy Description**:
  - **Writes.** The shared skeleton allows writes to `/private/tmp` and `/private/var/folders` for both profiles. `$TMPDIR` sits under `/private/var/folders`. But §2 says a CLI writes "only its CLI run folder". As a result, `fake_cli_cannot_write_outside_its_run_folder` passes or fails depending on where it writes.
  - **Reads.** Only `binary_dir` and `config_dirs` are readable under `$HOME`. Several installs need more:
    - interpreters and package folders elsewhere under `$HOME`, such as Volta (`cli_llm.rs:365`) and nvm, fnm and mise installs;
    - the hybrid arm's `current_exe()`, which lives in a worktree under `$HOME`.
    These may not start.
  - The plan doesn't say where the allow for the CLI's own `config_dirs` sits in the last-match-wins order, or whether `.gitconfig` applies to the CLI profile.
* **Rationale & Recommendation**:
  - Give `Profile::Cli` its own exact write list: the run folder, `/dev` and its `config_dirs`.
  - Decide which interpreter and package folders, resolved canonically, are readable. Add the parent of `current_exe()` when the hybrid arm is active.
  - Give the full rule order for the CLI profile.

### M6. Turning "each secret-check pattern" into Seatbelt rules would deny `/dev` - Severity: Major
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** `Line 1220`
* **Discrepancy Description**: `sensitive_path()` includes `^/(proc|sys|dev)/` and `^/etc/(shadow|sudoers|passwd)` (`src/permissions.rs:541`). A read deny on `/dev/` blocks `/dev/null`, `/dev/urandom` and the tty, which breaks every command in the sandbox.
* **Rationale & Recommendation**: List exactly which alternatives become Seatbelt rules, leaving out the `/proc`, `/sys`, `/dev` and `/etc` ones, and cover that list with `build_and_git_still_work_under_deny_default_reads`.

### M7. Existing fake-CLI tests break under the CLI profile - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 1219, 1223, 1321`
* **Discrepancy Description**:
  - `test_cfg` sets `cfg.home` to the tempdir (`cli_llm.rs:2024-2030`), and `fake_bin` writes the fake binary and the probe files into that same folder (`2008-2022`, `2293`, `2328`).
  - Once `cfg.home` is Refused, the Refused deny comes last and wins over the `binary_dir` allow, so on macOS the fake CLIs can't run.
  - `cancelled_chat_removes_its_temp_dir` (`3044-3080`) watches `<home>/tmp`. It isn't listed.
* **Rationale & Recommendation**: Have U2 give these tests a home that is separate from the binary and output folders, and list every affected test by name.

### M8. `CliLlm` has no `Config` or `Store`, and the version and login paths are incomplete - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 1159, 1171, 1180, 1184-1185, 1004`; spec `407-408`
* **Discrepancy Description**:
  - **Missing context.** These all need a `Config`, a `Store` or both:
    - `sandbox::wrap(cfg, …)` and `ReadRoots::of(cfg)`;
    - the kv `cli_version_ok` lookup;
    - the version cache.
    `CliLlm` copies only selected fields in `from_config` (`cli_llm.rs:1160-1198`).
  - **The API-key setting:**
    - `CliLlm` stores `allow_api_key: bool` (`1166, 1193`);
    - `set_settings` assigns a bool to `cfg.llm.cli_allow_api_key` (`server.rs:281-282`), which won't compile against `ApiKeyAllow`;
    - no route or Settings control writes the list form, `cli_subscription` or `allow_unattended_subscription`.
  - **Two spec cases with no plan item:**
    - the "no version match" case (spec 407), where the version output can't be parsed at all;
    - the "<name> is logged out: run `<login_command>`" messages for every candidate CLI, plus the login text each one prints.
* **Rationale & Recommendation**:
  - Add `cfg: Arc<Config>` and a `Store` handle to `CliLlm`. `llm::build` already rebuilds it on `update_config`.
  - Give the `POST /api/settings` body shapes for the new fields, and the Mac Settings controls that write them.
  - Treat "no version match" like "outside the range".
  - Give a login command and a logged-out detection pattern per candidate, marked UNVERIFIED where needed.

### M9. The "Provider trait" doesn't exist; the trait is `Llm`, and its wrappers must forward - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 1186, 1257`; spec `251`
* **Discrepancy Description**:
  - The model trait is `Llm` (`src/llm.rs:85`), not a `Provider` trait.
  - The App wraps every model in `SwappableLlm` (`app.rs:70, 142`), and `Cassette` wraps it too. Unless both forward `audit_identity`, every audit row gets the default identity.
  - No `AuditEntry` field (`store.rs:144-151`), `audit_log()` change or `/api/audit` change shows provider and CLI version, which CUJ 1 requires (spec 194).
* **Rationale & Recommendation**:
  - Name the trait `Llm`, give the default identity, and require forwarding in `SwappableLlm` and `Cassette`.
  - Add the two fields to the activity-log API and to the API fixtures.

### M10. The approvers can't compute the new "always" grant - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 1021-1024, 1058-1059, 1393`
* **Discrepancy Description**:
  - `ApprovalRequest` has no read target (`permissions.rs:287-299`). Even so, the web approver (`server.rs:408`), the terminal approver (`main.rs:168-178`) and Telegram each compute `can_always` and `always_rule` themselves.
  - Two cases are undefined:
    - what an `Always` answer does when `always_rule` returns `None` (`agent.rs:488-495`);
    - what "this conversation" stores for a too-broad folder. `grant_for_session` stores `always_rule(...)` (`agent.rs:117-124`), which is now `None`.
* **Rationale & Recommendation**:
  - Add `always_rule: Option<String>` to `ApprovalRequest`, computed once in `execute_call`, and have every approver use it.
  - Treat `Always` with `None` as Once.
  - Define the conversation grant for a too-broad folder.
  - List every caller of `always_rule`.

### M11. The shell sandbox ignores conversation grants, which contradicts §1 - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `style`
* **Affected Scope:** `Line 1208`; spec `323`
* **Discrepancy Description**: §1 says the shell sandbox "reads the same rules", right after it describes "allow for this conversation". Item 26 reads only standing rules, and leaves out kv `grants:<session>` (`agent.rs:98-100`).
* **Rationale & Recommendation**: Either include the session's grants or change §1. Product call: ask the owner.

### M12. The new `cfg` parameter can't reach ambient observation and source indexing as written - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Line 1090`
* **Discrepancy Description**:
  - `recent_changes` is called only from `ambient::observe(store, workspace)` (`ambient.rs:90-102`). That function runs inside `spawn_blocking` closures (`app.rs:419-420`, `agent.rs:272-276`), and tests call it directly (`app.rs:950-1128`).
  - `sources.rs:253` sits inside the private `scan_folder`, which also runs in `spawn_blocking` (`274-285`).
  - A borrowed `&app.cfg()` can't be moved into those closures.
* **Rationale & Recommendation**: Specify `observe(store, workspace, cfg: Arc<Config>)` and `scan_folder(store, folder, &Config)`, move an `Arc` into each closure, and list every caller.

### M13. The subscription hold is wired through the wrong call paths - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `style`
* **Affected Scope:** `Lines 1281-1289`
* **Discrepancy Description**:
  - **Call paths.**
    - `run_schedule` calls `app.run_now(id)` (`server.rs:708-716`, `app.rs:315-319`), not `run_job` directly.
    - `run_triggers` also calls `run_job` (`app.rs:366`).
    - The scheduler reaches `run_job` through `run_due_schedules` (`app.rs:187, 690-713`).
    - Automation `ask` and `agent` steps reach the model through `JobHost::ask` and `JobHost::agent` (`app.rs:735-748`), not through `workflow.rs`.
  - **Status.** `run_job` returns `bool`, and its caller records only "ok" or "failed" (`app.rs:187-188, 311`). So `held-subscription` can't be recorded.
  - **Logic.** "is true when:" over four bullets doesn't say whether all four conditions must hold.
* **Rationale & Recommendation**:
  - Specify `run_now(id, attended: true)`, with `run_due_schedules` and `run_triggers` passing `false`.
  - Put the hold check in `JobHost::ask` and `JobHost::agent`.
  - Give `run_job` a status enum.
  - Define what a workflow does with a held step.
  - Write "only when all four hold".

### M14. The risk-notice block misses model calls that bypass `run_turn` - Severity: Major
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** `Lines 1375, 1425`; spec `174, 558`
* **Discrepancy Description**: These call `llm.chat` directly, so unattended model use goes on before the notice is accepted:
  - `JobHost::ask` (`app.rs:735-742`);
  - `reflect_if_due` (`app.rs:462`);
  - `proactive_tick` (`app.rs:582`).
* **Rationale & Recommendation**: Add the check to each of these, or enforce it once in `SwappableLlm::chat`.

### M15. Existing tests that U1 to U3 break are still missing from the lists - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 1143-1150, 1321, 1428-1445`
* **Discrepancy Description**:
  - **U3** (`server.rs:1169-1245`):
    - `dropped_chat_stream_aborts_only_its_original_run_and_clears_approvals` asserts the behaviour U3 removes, and builds `ServerState` with `next_run`;
    - `stalled_chat_reader_cannot_accumulate_unlimited_events` depends on `CHAT_EVENT_CAPACITY`.
  - **U1, M-6:** `proactive_levels_suggest_then_take_over_within_bounds` (`app.rs:992-1047`) expects a takeover write that will now park.
  - **U1, `agent.rs`:**
    - `the_agent_cannot_rewrite_its_own_settings_unasked` (`agent.rs:957-969`) expects `Privileged`;
    - `every_built_in_tool_declares_what_it_changes` (`794-795`) needs rows for the two new tools.
  - **Docs:** `SECURITY_THREAT_MODEL.md:68` documents the old stream backpressure.
* **Rationale & Recommendation**: List each test with its new expected behaviour, or mark it for deletion and name its replacement.

### M16. The bus's run semantics and several event and route shapes are incomplete - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 1368-1399, 1465`
* **Discrepancy Description**: The plan doesn't say:
  - who publishes `done`, `error`, `cancelled` and `learned` to the bus, or when a filtered `/api/chat` stream ends (today `learned` follows `done`, `server.rs:495-505`);
  - what `subscribe(since)` does when `since` is past the current `seq`. That happens after a core restart, because `seq` starts again at 1;
  - how `event_json` (`server.rs:425-434`) reaches `App::sink_for`;
  - the values of `approval_resolved.decision`;
  - the JSON shape of `GET /api/grants`;
  - how the deterministic `events-error.jsonl` fixture stream is produced, which U4's tests decode.
  - `JobHost::agent` also uses `null_sink()` (`app.rs:747`), which isn't listed.
* **Rationale & Recommendation**:
  - Specify each of these.
  - Treat a `since` past the current `seq` as a `resync`.
  - Give `approval_resolved.decision` the same values as `POST /api/approve`.
  - Give `/api/grants` as `[{rule, kind}]`.
  - Produce `events-error.jsonl` from a demo turn with a scripted failure.

### M17. `require-tests.sh --lib` can't see a test placed in the `heron` bin - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `style`
* **Affected Scope:** `Lines 896, 1417-1421, 1443`; table `1651`
* **Discrepancy Description**:
  - `print_endpoint_line_has_port_token_and_owner` tests a `main.rs` feature, and `main.rs` is a separate bin target with its own test module (`main.rs:759-792`). If the test goes there, `--lib -- --list` won't list it, and U3's check fails.
  - The matching rule is unstated. `every_registry_config_dir_is_refused` is a prefix of a U2 test name.
* **Rationale & Recommendation**:
  - Put the line formatter in `server.rs` (for example `server::endpoint_line(&Started)`) and test it there, or use `--lib --bins`.
  - State that a name must equal the last `::` component of a listed test exactly.

### M18. A measurement unit can't repair a failure whose cause is in `src/` - Severity: Major
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** Table `1649-1650, 1653`; `1339`
* **Discrepancy Description**:
  - U2c's check requires claude-cli plus one other candidate to pass (G4). A containment failure caused in `src/` can't be fixed from `eval/containment/**`; one example is an interpreter outside `binary_dir` (M5).
  - U2w waits on U2c, so the chain stalls with no defined way out.
* **Rationale & Recommendation**:
  - Let U2c pass once its results are recorded, and move the G4 gate to U2w's check or to `verify`.
  - Alternatively, document that a U2c failure reopens U2 as a new skein node.

### M19. The Tier 1 promotion rule leaves out conditions stated elsewhere - Severity: Major
* **Reviewer Lens:** `style`, `implementation-readiness`
* **Affected Scope:** `Lines 1117, 1336, 1476-1479`
* **Discrepancy Description**:
  - **Missing conditions.** U2w promotes "each candidate that passed". It leaves out G4's "gemini-cli only if antigravity-cli fails", and line 1117's "folders confirmed".
  - **Experimental flags.** `experimental` is already `false` on every candidate, and on cursor-cli, copilot-cli and qwen-cli, which aren't candidates (`cli_llm.rs:286-295`). So "stay experimental" changes nothing.
  - **Finding config folders.** The two methods given (1117, 1336) diff `$HOME` before and after a run, and a diff can't find folders that already exist.
* **Rationale & Recommendation**:
  - State both promotion conditions.
  - Set `experimental = true` on every entry that isn't Tier 1, and assert it in `tier1_candidates_have_ranges`.
  - Find config folders by logging in under a fresh temporary `HOME`.

### M20. U3a links to threat-model entries that don't exist - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `style`
* **Affected Scope:** `Lines 951, 959-962`; table `1646`
* **Discrepancy Description**:
  - No `C35-8` or `R-1` entry exists in `docs/engineering/SECURITY_THREAT_MODEL.md` or `REMEDIATION_MATRIX.md` (grep). The matrix uses `C1…`-style ids.
  - The anchor `#c35-8-…` is a placeholder, and the section titles aren't given.
  - The plan doesn't say whether G2's "reads outside the working folder now ask" notice goes into the register.
  - This depends on whether #35 adds the entry.
* **Rationale & Recommendation**:
  - Say whether U3a creates the C35-8 and R-1 rows, and in what format.
  - Give the exact headings and anchors.
  - Place the G2 notice text.

### M21. The skein backend's allowlist stops agents from doing the work assigned to them - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `style`
* **Affected Scope:** `Lines 750, 822, 922`
* **Discrepancy Description**:
  - U0 is told to write the EVE config from `eve chat --help` and to "run the audit once" with genesis. Neither command is allowed.
  - The agents for U0, U3a, U6, U6b, U7, U7b and `verify` can't run their own checks: `bash eval/run-core-tasks.sh`, `build-app.sh`, `actionlint`, `sqlite3`, `grep` and `test` are not allowed.
* **Rationale & Recommendation**: Either add per-unit commands to the backend, or move the UNVERIFIED discovery steps into completion commands. State that completion commands run outside the agent's allowlist.

### M22. The CI and release steps rely on secrets, variables and history that don't exist - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 1581, 1609-1620`
* **Discrepancy Description**:
  - **Secrets.** `release.yml:65-81` is the macOS job and references no secrets. No secret appears anywhere in the file.
  - **Title variable.** The title uses `$VERSION`, but that step has `$RELEASE_TAG` (`release.yml:164`).
  - **Shallow checkout.** The `macos-app` job's upgrade test needs `git worktree add … 9253959`, which the default shallow checkout (`ci.yml:20`) can't reach.
  - **Commit id.** `9253959` couldn't be confirmed from the refs (only `release/v0.3.1-prep` at `eb9a8f95…` is visible).
* **Rationale & Recommendation**:
  - Name every new secret (`MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `MACOS_CODESIGN_IDENTITY` and the three ASC values), and make creating them an owner precondition.
  - Use `$RELEASE_TAG`.
  - Set `fetch-depth: 0`, or cache the 0.3.1 binary.
  - Confirm the commit id.

### M23. The `-D warnings` gates run on an unpinned toolchain - Severity: Major
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** Table `1647-1659`; `1617, 1682`
* **Discrepancy Description**: `ci.yml:22` runs `rustup update stable`, and the repository has no `rust-toolchain` file (checked). A clippy release that arrives mid-build can fail a unit's check with a lint in code outside its scope.
* **Rationale & Recommendation**: Pin the toolchain with a root `rust-toolchain.toml`, recorded as a second single-file exception or set up before U1, or record the clippy version in the preconditions.

### M24. U4's build output isn't git-ignored, and some UI decisions are open - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `style`, `synthesizer`
* **Affected Scope:** `Lines 1494-1508`; table `1654, 1657`
* **Discrepancy Description**:
  - **Build output.** `.gitignore` lists only `target/` and `dist/`. `swift build` writes `macos/HeronMac/.build/`, so skein may commit it, both in U4 and during U7's `build-app.sh` run.
  - **Open UI decisions:**
    - navigation (sidebar or tabs);
    - the theme list;
    - the `avatarStyle` and `accentColor` values;
    - what "Prep my day" sends;
    - the menu bar extra's contents.
  - **Sandbox status.** CUJ 2's risk notice shows the sandbox status, but the first-run flow leaves it out.
* **Rationale & Recommendation**:
  - Add `macos/HeronMac/.gitignore` with `.build/` to U4's files.
  - Pin those UI choices, or say the implementer chooses them.
  - Add the sandbox status to the first-run flow.

### M25. Nothing checks that the regression tests fail on the old commit - Severity: Major
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** `Line 1136`; spec `100` (G3)
* **Discrepancy Description**: G3 requires each regression test to fail on the old commit. No unit or `verify` step shows that they do.
* **Rationale & Recommendation**: Add a `verify` step that runs the named `m*` tests at the baseline commit and expects them to fail, or record a manual procedure.

### M26. `every_prompt_block_is_trusted_or_taints` can pass without testing anything - Severity: Major
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** `Lines 1060, 1123`
* **Discrepancy Description**: With an exhaustive `match` on `PromptSource`, walking the registry checks nothing the compiler doesn't already enforce.
* **Rationale & Recommendation**: Render `system_prompt` with every source filled in. Assert that every header maps to a registered source, and that the removed blocks are absent.

### M27. The CLI's wrapping on Linux is unspecified - Severity: Major
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** `Lines 1180, 1225`
* **Discrepancy Description**: On Linux, `wrap()` returns Bubblewrap arguments (`sandbox.rs:85-99`). The plan doesn't say whether the CLI child becomes bwrap-wrapped on Ubuntu CI and Linux hosts, which would change today's behaviour against Q11.
* **Rationale & Recommendation**: State "Seatbelt only" for `Profile::Cli`, or define the Linux behaviour and its tests.

### M28. The "working folder inside a Refused path" refusal has a test but no implementation - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `style`
* **Affected Scope:** `Lines 1128, 1224`; spec `320`
* **Discrepancy Description**: No item says where the error "the working folder is inside <path>, which Heron never reads" is raised, or what type it has. Candidates are `App::build`, `run_turn`, session creation and the shell.
* **Rationale & Recommendation**: Name the function and the error type, and say which surfaces show it.

### M29. The G9 step in `verify` can't fail, and doesn't check the window - Severity: Major
* **Reviewer Lens:** `style`, `implementation-readiness`
* **Affected Scope:** `Lines 913, 1634`; spec `118`
* **Discrepancy Description**:
  - `owner-use.sh` always exits 0, and nothing compares `qualifying_days` with 12.
  - Nothing checks that the window is the 14 consecutive days G9 requires.
* **Rationale & Recommendation**: Add `--min <k>` to `owner-use.sh` (exit 1 below it), and have `verify` check that the range is exactly 14 days.

### M30. The `Policy::new` sentence can be read as dropping tool-wide rules - Severity: Major
* **Reviewer Lens:** `style`
* **Affected Scope:** `Line 1028`; spec `324`
* **Discrepancy Description**: "keeps too-broad `read:` rules out of the matcher, and records them, together with the tool-wide rules…" can be read as keeping the tool-wide rules out of the matcher too. §1 says they still cover their other effects.
* **Rationale & Recommendation**: Split the sentence. Tool-wide rules stay in the matcher for non-OutsideRead effects, and both kinds are listed in `rule_notices()`.

---

### N1. U6b and U6 checks are weak - Severity: Nit
* **Reviewer Lens:** `implementation-readiness`, `synthesizer`
* **Affected Scope:** Table `1655, 1656`
* **Discrepancy Description**: `README.md:135` already contains "outside the working folder", so U6b's third grep passes without the mode-table edit. U6's check mostly uses `test -s`.
* **Rationale & Recommendation**: Grep for one exact new sentence per required change.

### N2. The `server.rs:1070` comparison never fires - Severity: Nit
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Line 1091`
* **Discrepancy Description**: A read classification never returns `Privileged` (`permissions.rs:598-605`), so "treat `Refused` like `Privileged`" keeps a check that never fires.
* **Rationale & Recommendation**: Use `matches!(effect, Refused | SensitiveRead)`.

### N3. Callers of `add_allow` will get unused-`Result` warnings - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Line 1027`
* **Discrepancy Description**: `agent.rs:492`, `app.rs:109, 1214` and `permissions.rs:996-997` will warn on the unused `Result`, and clippy's `-D warnings` turns that into an error.
* **Rationale & Recommendation**: List these callers.

### N4. `FnTool` literals in `browser.rs` need the new field - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Line 1036`
* **Discrepancy Description**: `src/browser.rs:537-577` builds `FnTool` literals, which need the new `target` field.
* **Rationale & Recommendation**: List the file.

### N5. A test citation is off - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Line 1149`
* **Discrepancy Description**: `fs.rs:1224` is the symlink-to-`.ssh` test, which stays `SensitiveRead`. The Heron-home assertions are at `1235-1238`.
* **Rationale & Recommendation**: Cite `1235-1238`.

### N6. "Every test helper" misses inline set-up - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Line 1444`
* **Discrepancy Description**: About 12 inline `App::assemble` sites aren't helpers: in `app.rs`, `sources.rs:555`, `telegram.rs:339` and `server.rs:1183`.
* **Rationale & Recommendation**: Write "every test", or list the sites.

### N7. The sandbox-off tests don't exist on Windows - Severity: Nit
* **Reviewer Lens:** `codebase-details`, `style`
* **Affected Scope:** `Lines 1295, 1302`
* **Discrepancy Description**:
  - `shell.rs`'s test module is `#[cfg(all(test, unix))]` (`shell.rs:192`), so `shell_refused_when_sandbox_off` doesn't exist on Windows.
  - The two sandbox-off tests sit under a heading that says the tests skip elsewhere.
  - `sandbox::available()` is called `active()` elsewhere.
* **Rationale & Recommendation**: Fix the claim, the grouping and the name.

### N8. The upgrade fixture's `heron run` writes into the current folder - Severity: Nit
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Line 1583`
* **Discrepancy Description**: `heron run` uses the current folder as its working folder, and the `<prompt>` it runs is unspecified.
* **Rationale & Recommendation**: Pass `-C <tmp>`, and give a prompt that makes a demo tool call (`llm.rs:633-636`).

### N9. The `ReadRoots` cache key misses environment variables - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Line 1004`
* **Discrepancy Description**: `CLAUDE_CONFIG_DIR` and `CODEX_HOME` add Refused roots but aren't part of the cache key, so a later change to them isn't picked up.
* **Rationale & Recommendation**: Add them to the key.

### N10. `read_target` has two types - Severity: Nit
* **Reviewer Lens:** `style`, `codebase-details`
* **Affected Scope:** `Line 984`; spec `283`
* **Discrepancy Description**: The plan gives `Option<&'a Path>`; the spec gives `Option<PathBuf>`.
* **Rationale & Recommendation**: Use one type.

### N11. The runner's exit codes overlap - Severity: Nit
* **Reviewer Lens:** `style`, `implementation-readiness`
* **Affected Scope:** `Lines 823, 868-870`
* **Discrepancy Description**:
  - Exit 2 means both a usage error and "the eve driver can't run".
  - Exits 0 and 1 overlap, because an errored run was still "attempted".
* **Rationale & Recommendation**: Define each code once.

### N12. Task-file details - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Lines 765, 783, 786, 791, 881`
* **Discrepancy Description**:
  - The probe id grammar allows only `p1`, but the check requires "at least one probe".
  - t02 gives two strings to one `file_contains`.
  - t07's `\b` needs JSON escaping.
  - "(shortened)" leaves the actual prompt text unclear.
* **Rationale & Recommendation**: Fix each one.

### N13. `check_gate.py` doesn't check the run's conditions - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Lines 889-890`
* **Discrepancy Description**:
  - Default mode doesn't check `provider`, `driver` or `mode`, so a `demo`/`direct` summary could pass G7.
  - `--spike` doesn't check `build`.
* **Rationale & Recommendation**: Require these fields.

### N14. The baseline commit is defined two ways - Severity: Nit
* **Reviewer Lens:** `style`, `implementation-readiness`
* **Affected Scope:** `Lines 937-938, 1627`; spec `90`
* **Discrepancy Description**: `heron_commit` will be a `feat/heron-v1` commit, not "the `main` commit". `verify` doesn't name the field it reads `BASE` from.
* **Rationale & Recommendation**: Align the definition with what is recorded, and name the field.

### N15. Approvals are counted two ways - Severity: Nit
* **Reviewer Lens:** `style`, `implementation-readiness`
* **Affected Scope:** `Line 818`; spec `439`
* **Discrepancy Description**: `asks` counts `approval` events, while the spec counts approvals from `audit` rows.
* **Rationale & Recommendation**: Pick one source.

### N16. The question-to-unit mapping is incomplete - Severity: Nit
* **Reviewer Lens:** `style`, `implementation-readiness`
* **Affected Scope:** `Line 754`
* **Discrepancy Description**:
  - Q11 is needed by U3 but isn't listed for it.
  - Q4 isn't listed for U0 or U3a.
  - Q5 is assigned to U2, but no unit runs the reduced gate or the per-provider latency report.
* **Rationale & Recommendation**: Complete the mapping, and give Q5 a unit or drop it.

### N17. The register limit is counted two ways - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Lines 956, 1636`
* **Discrepancy Description**: The limit is "3 entries besides C35-8", but `verify` counts `## R-` headings.
* **Rationale & Recommendation**: Count the same way in both places.

### N18. Headings and the re-check scope are inexact - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Lines 748, 964, 1163, 1242, 1364`
* **Discrepancy Description**:
  - The U1 heading claims "M-1 to M-4".
  - Some item headings leave out files the items change: `llm.rs`, `config.rs`, and the callers in `agent.rs` and `mcp.rs`.
  - Only three files' line numbers, and not the clippy count, are re-checked after the merges.
* **Rationale & Recommendation**: Match the headings to the content, and re-check every citation and the clippy count after the merges.

### N19. The date regex uses `\d` in bash - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Line 902`
* **Discrepancy Description**: `\d` isn't supported in bash's ERE on macOS, and the exit code for a bad date isn't given.
* **Rationale & Recommendation**: Use `[0-9]`, and give the exit code.

### N20. The `409` is overloaded, and the run-folder `TMPDIR` is unclear - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Lines 1178-1179, 1406`
* **Discrepancy Description**: The `409` meaning is overloaded (see B3). It is also unclear whether `$TMPDIR` in the run-folder path means the parent's value.
* **Rationale & Recommendation**: Say "the parent's `$TMPDIR`".

### N21. The Cline bullet contradicts itself - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Line 1197`
* **Discrepancy Description**: It keeps `needs_allow_agent_tools`, "unless U2c's canary passes", but no unit applies that result.
* **Rationale & Recommendation**: Say that U2w drops the flag if the canary passes with tools off.

### N22. Parallel measurement shares one login, and testteeth's scope is unstated - Severity: Nit
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** `Lines 1631, 1663`
* **Discrepancy Description**:
  - U2c and U2s run at the same time on the same claude-cli login, so they compete for quota and rate limits.
  - testteeth's language and feature scope (Rust, Python, the `desktop` feature) isn't stated.
* **Rationale & Recommendation**: Run U2c before U2s, or note the cost. State testteeth's scope.

---

## Dismissed or not verifiable

- **Not verifiable read-only:**
  - the "57 clippy errors" count, and whether `field_reassign_with_default` fires in `tests/` (B2 gives the command that settles it);
  - that commit `9253959` exists and is data format 2;
  - the claude-cli flags (`--mcp-config`, `--strict-mcp-config`, `--max-turns`);
  - the EVE and genesis 0.4.0 formats;
  - `swift build --arch` output paths;
  - Seatbelt exec semantics;
  - how skein matches a file-path blast radius (see the Q19 row in the audit).

  The plan marks most of these UNVERIFIED itself. They are reported above as gaps where they block a check, not as confirmed defects.
- **Downgraded from Blocker to Major:**
  - The `python3 -I` import failure (M1), because the unit can fix it inside its own scope without a design change.
  - The fake-CLI test breakage (M7) and the missing test lists (M15), because the fix lies within the breaking unit's own `src/**` scope.
- **Dismissed:**
  - **U2s and U2c running alongside U3 in eval/.** Their scopes are disjoint from `src/**`, so this is not a conflict.
  - **The style lens's list of minor rewordings** (N1, N22, N45 to N62 in its raw report: wording of "Placement", "gate", "justified", the `CoreProcess` sentence, the snapshot tolerance phrasing, the Persona/Personalization naming, and others). These are readable as written, and an implementer wouldn't build the wrong thing from them. They are folded into N18 where they touch headings, and otherwise dropped.
  - **The anchor check.** All ten internal anchors resolve. The only broken link is `#c35-8-…`, which is covered in M20.
