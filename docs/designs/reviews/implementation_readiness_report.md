# Goldfish Review Report

**Target:** `docs/designs/heron-v1.md` (`Lines 715-1080`, "## Detailed Implementation Plan" to end of file, at commit ff5a31d)

**Stage:** 9, implementation-readiness test (Mode: REVIEW)

**Reviewers:**
- `egm-implementation-readiness` and `egm-codebase-details`, both against `implementation-readiness-standard.md`.
- `egm-style`, against its persona's default.
- The Work units rule audit was done by the synthesizer (lens `synthesizer`).

**Verdict: not ready.** The table has every required column on all 13 rows, but three problems remain:
- 4 units can't pass their own check in the declared order.
- 2 scopes are not directory globs.
- 5 units' globs don't cover what they write or need.

Many design choices are also still left to the implementer.

**Counts after deduplication:** 8 Blockers, 28 Majors, 12 Nits.

**How the findings were checked:** for each Blocker, I checked the repository fact it rests on:

| Blocker fact | Where |
|---|---|
| `CliSpec` has no `config_dirs` | `src/cli_llm.rs:106-128` |
| `STEPS = [to_v1, to_v2]` | `src/store.rs:161` |
| `docs/ACCEPTED_RISKS.md` doesn't exist, but the spec requires `include_str!` | spec line 526 |
| Allow rules match `"{tool}:{summary}"` | `src/permissions.rs:220` |
| The read tools reach `classify_path` through `read_effect` | `src/tools/fs.rs:16-26` |
| Shell tests turn the sandbox off | `src/tools/shell.rs:204, 247, 299` |
| CI runs on Linux, macOS and Windows | `.github/workflows/ci.yml:17` |
| The demo model is chosen by `base_url = "demo"` | `src/llm.rs:591` |
| Routes are nested under `/api` | `src/server.rs:165` |
| `Cargo.toml` has `[features]` but no `semver` or `spike` | `Cargo.toml` |

I also checked these facts behind Major findings:

| Major fact | Where |
|---|---|
| The existing snapshot is named `before-upgrade-from-v{n}` | `src/store.rs:533` |
| `save_automation` is not in `SERVED_TOOLS` | `src/mcp.rs:232` |
| CI has no clippy, fmt or testteeth step | `.github/workflows/` |
| The release is titled "(beta)" with `--prerelease` | `release.yml:164` |
| `build-app.sh` builds `--features desktop` | line 9 |
| `build-app.sh` overwrites `Info.plist` | line 26 |
| `build-app.sh` signs ad hoc | line 30 |

The remaining Major and Nit evidence comes from the reviewers' reads, and I did not re-check it line by line.

**Precondition not met:** PRs #26 and #35 are not in this tree. There is no `upcoming_events`, and the calendar is still read in `src/agent.rs:285-291`. Line references into `agent.rs` and `telegram.rs` will move once both merge.

---

## Work units rule audit (lines 1047-1065)

| Rule | Result |
|---|---|
| id, what to build, depends on, check and model role on every row | **Pass.** All 13 rows have them. Every model role is a valid role line (`feature, refactoring`, `hardest tasks`, `judgment and prose`). |
| Scope is one directory glob | **Fail.**<br>- Two scopes are single files, not directory globs: U6b (`README.md`) and verify (`scripts/verify-v1.sh`).<br>- Five units have globs that don't cover what they write or need: U1, U2, U2w, U3 and U2s. See B8. |
| Check is a shell command that exits 0 | **Pass on form.** Three caveats:<br>- Two checks can pass while running zero tests (M24).<br>- U0's check uses `--provider demo`, a flag of U0's own runner whose mapping to `base_url = "demo"` is never specified (B6).<br>- U0's check exits 0 on canned demo replies only if the runner ignores pass-rule failures, and the plan never states the runner's exit semantics (B5). |
| Check can pass with only that unit's files built | **Fail** for 4 units:<br>- U1 needs U2's `config_dirs`, `sessions.origin` and sandbox (B1).<br>- U3 needs U6's `docs/ACCEPTED_RISKS.md` (B2).<br>- U2w greps `Cargo.toml`, which is outside its scope (B8).<br>- U2s needs a topology option in U0's runner, which it can't edit (B8, M25). |
| Dependency order is buildable | **Fail.** The declared graph is acyclic, but:<br>- U3 needs a file that only U6 creates, and U6 comes after U2w, which comes after U3. In practice that is a cycle (B2).<br>- verify depends on U5, which has no content (B7).<br>- verify's G7 run comes after U3's risk-notice gate, and no step pre-accepts the notice (M16). |

---

## Prioritized Findings (Blockers -> Major -> Nits)

### B1. U1 uses work that only U2 delivers - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `synthesizer`
* **Affected Scope:** `Lines 775, 814, 832-833, 840` vs `850, 890-893`; table `1052-1053`
* **Discrepancy Description**: U1's `cargo test` can't pass at U1, because three things it uses arrive only in U2:
  - **Registry config folders.** U1 builds Refused from "every registry entry's `config_dirs`" (775), and its test `every_registry_config_dir_is_refused` (833) relies on that. The field is added to `CliSpec` only in U2 (850), and `src/cli_llm.rs:106-128` has no such field today.
  - **Session origin.** Item 10 (814) has U1 set `sessions.origin`. That column is added only by U2's `to_v3` (893), and today `src/store.rs:161` is `STEPS = [to_v1, to_v2]`.
  - **Sandbox denial.** The M-4 test (840) requires a sandbox denial. Deny-default sandbox reads are U2's item 15, and today's profile allows every read except a short list (`src/sandbox.rs:66-72, 101`).
* **Rationale & Recommendation**:
  - Either move the `config_dirs` field (with its per-entry values) and the `sessions.origin` column into U1, or move those parts of U1 into U2.
  - Split the M-4 test. Keep the `read_file` half in U1, and move the sandbox half to U2 next to `sandbox_denies_secret_matches`.

### B2. U3 can't compile: the risk notice embeds a file that U6 creates later - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `synthesizer`
* **Affected Scope:** `Lines 927, 981-984`; table `1056, 1059`; order `1065` (spec lines 522 and 526)
* **Discrepancy Description**: The spec requires `docs/ACCEPTED_RISKS.md` to be compiled into the core with `include_str!` (spec line 526). U3's `/risk-notice` route (item 20) serves that file. But:
  - U6 creates the file. U6 depends on U2w, and U2w depends on U3.
  - The file doesn't exist in the repo today, so `include_str!` fails and U3's check can't pass. In practice this is a dependency cycle.

  Item 20 also leaves three things unstated:
  - the `include_str!` path;
  - the rule that the file's first line is `Version: <integer>`;
  - where the `RiskNoticeNotAccepted` error type lives.
* **Rationale & Recommendation**:
  - Add a small `docs/**` unit ahead of U3, for example "U3a: placeholder risk register", whose first line is `Version: 1`. U6 then fills it in. This keeps U3's scope a single glob, which letting U3 write under `docs/` would break.
  - In item 20, give the path `include_str!("../docs/ACCEPTED_RISKS.md")` and the parsing rule.

### B3. U2's sandbox-off refusal breaks existing tests and the Windows and Linux CI runs - Severity: Blocker
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 853, 869, 878-879`
* **Discrepancy Description**: The refusal collides with existing tests and CI:
  - The tests at `src/tools/shell.rs:204, 247, 299` turn the sandbox off and expect commands to run.
  - The fake-CLI tests (`src/cli_llm.rs:2222` onward) go through `CliLlm::chat`.
  - `ci.yml:17` and `release.yml:83-125` run `cargo test --locked` on Windows and Linux. Windows has no sandbox (`src/sandbox.rs:34-43`), and Linux has one only when `bwrap` works.

  The plan also never says whether shell and CLI providers are now refused on Windows and Linux. That would be a large product change on platforms the spec calls "still building".
* **Rationale & Recommendation**:
  - State which platforms the refusal applies to, for example `cfg!(target_os = "macos")` only.
  - List the existing tests to update.
  - Add a test seam, such as a fake sandbox, so the shell and fake-CLI tests still run on every CI platform.

### B4. Folder read grants can't match today's allow rules, and old tool-wide rules would open the Ask tier - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 777-779, 834`
* **Discrepancy Description**: The new grants can't work, and the old rules would bypass them:
  - **Rule format.** Allow rules are globs matched against `"{tool}:{summary}"` (`src/permissions.rs:220, 251, 272-280`). For the read tools the summary is the raw, unresolved `path` argument (`src/tools/fs.rs:12-14`). For `search_files` and `find_files` it isn't a path at all (`fs.rs:837, 847`). So a stored `read:/x/**` can never match `read_file:...`.
  - **Old rules.** `Effect::OutsideRead` is deliberately non-high, so any existing tool-wide rule would allow every Ask-tier read and silently defeat G2. Such rules include:
    - `read_file:*`, which the Settings "allow" switch writes (`src/server.rs:926-930`);
    - `{tool}:*`, which today's `always_rule` returns (`permissions.rs:353-360`);
    - bare `*` or `*:*`.
  - The broad-grant list (779) names only path forms, not these.
* **Rationale & Recommendation**: Specify five things:
  - the OutsideRead match key, for example `read:<canonical resolved path>` matched against the folder glob, for all four read tools;
  - that only `read:` rules can grant OutsideRead;
  - that `{tool}:*`, `*` and `*:*` never grant it;
  - what the Settings "allow" switch does for the four read tools;
  - that these rule sets are added to `promises_that_hold_whatever_the_settings`.

### B5. The G7 task gate and the U0 runner's output and exit rules are not defined - Severity: Blocker
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `style`, `synthesizer`
* **Affected Scope:** `Lines 729-755`; table `1051`
* **Discrepancy Description**: Only task 10 and probe P1 are described. The plan leaves out:
  - the other 9 tasks;
  - the file format of `core-tasks.md`;
  - the grammar of a "machine-checkable pass rule";
  - the `summary.json` schema that `check_gate.py` reads;
  - the exit semantics of `run-core-tasks.sh`. U0's check passes on demo replies only if a failed pass rule doesn't make the runner exit non-zero.

  `check_tasks.py`, `run-core-tasks.sh`, the baseline, the 50-run spike and G7 all depend on these.

  The text has two further problems:
  - Line 750 is broken. "It writes a JSON summary with: The summary also records..." puts two unrelated sentences between the colon and its list.
  - "Median time to first token" doesn't say whether it is per task or per run.
* **Rationale & Recommendation**:
  - Give all 10 tasks. For each: id, prompt, fixtures, approve and deny pairs, and an exact pass-rule expression.
  - Define the file grammar and how task 10 is marked.
  - Define the `summary.json` keys and the runner's exit codes.
  - Rewrite 750-753 so the bullets follow the colon, add approvals per task as a fourth bullet, and put the `--resume` sentence after the list.

### B6. The U0 runner can't drive Heron on today's `main` - Severity: Blocker
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `synthesizer`
* **Affected Scope:** `Lines 744, 750`; table `1051`
* **Discrepancy Description**: Four parts of the runner don't fit today's code:
  - **Approvals.** A live approval's id travels only inside that one `/chat` SSE stream (`src/server.rs:407-411`), and `/approvals` lists only parked approvals. The plan doesn't say whether EVE or the runner reads the stream and posts the answer.
  - **Token.** `--print-endpoint` arrives only in U3. Today the token appears only inside the printed URL (`src/main.rs:679`, `src/server.rs:190`), and the plan doesn't say how the runner gets it.
  - **Route prefix.** Routes live under `/api` (`src/server.rs:165`), but the plan writes `/chat`.
  - **Provider flag.** `--provider demo` is a flag of the runner U0 itself creates, but nothing says what config it produces. No provider has the id `demo`. The demo model is chosen by `base_url = "demo"` (`src/llm.rs:591`).
* **Rationale & Recommendation**: Specify:
  - how the disposable home is started and the token read, for example by parsing the URL line;
  - who answers approvals, and over which `/api/...` route;
  - the exact config each `--provider` value produces;
  - the `/api/` prefix on every route.

### B7. U5 is empty, and the `verify` unit depends on it - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `synthesizer`
* **Affected Scope:** `Lines 972-977`; table `1058, 1063`
* **Discrepancy Description**: U5 lists no files, steps or tests. The plan itself says "This unit isn't ready until Q8...". The verify row depends on "U0-U7b", which includes U5, so the final gate can't be scheduled.
* **Rationale & Recommendation**: Either remove U5 from this plan and from verify's dependencies, and add it later as its own design delta once Q8 is answered, or fill it in now. Either way, list verify's dependencies explicitly (see N7).

### B8. Unit scopes don't cover the files those units must write - Severity: Blocker
* **Reviewer Lens:** `synthesizer`, `codebase-details`
* **Affected Scope:** Table `1051-1063`; items `13, 14, 16, 18, 22`
* **Discrepancy Description**: The rule requires one directory glob per unit, and a unit's check must pass with only its own files built. Seven units break this.

  Five units' globs don't cover what they write or need:
  - **U1** (`src/**`) changes `tests/integration.rs` (item 13).
  - **U2** (`src/**`) creates `tests/cli_canary.rs` (item 18). It must also add `semver` and a `spike` feature to `Cargo.toml`, which has a `[features]` table but no `semver` and no `spike`.
  - **U2w** (`src/**`) runs a check that greps `Cargo.toml`, and it must delete the `spike` line there.
  - **U3** (`src/**`) changes `tests/integration.rs` (item 22).
  - **U2s** (`eval/spike/**`) needs a topology option in `eval/run-core-tasks.sh`, which belongs to U0's `eval/**`. Today the runner can't select the hybrid arm.

  Two scopes are single files, not directory globs:
  - **U6b:** `README.md`.
  - **verify:** `scripts/verify-v1.sh`.
* **Rationale & Recommendation**: A scope of "`src/`, `tests/` and `Cargo.toml`" would itself break the one-glob rule, so these are the options that comply:
  - **Tests:** give each `src/**` unit's tests to a paired `tests/**` unit that depends on it. Because the `src/**` units already run one after another, the `tests/**` units can be chained the same way. Alternatively, keep the new tests inline in `src/` as `#[cfg(test)]` modules and drop the `tests/` edits.
  - **Cargo changes:** add a small root-manifest unit, whose scope is `Cargo.toml`, ahead of U2 for `semver` and the `spike` feature. Have U2w's removal of `spike` go through the same unit. If the owner accepts a file scope for manifests, say so as an exception to the rule.
  - **Runner flag:** give U0's runner a `--topology envelope|hybrid` option, or move that change into a unit that owns `eval/**`.
  - **Single-file scopes:** give U6b and verify directory globs, for example a root-docs unit for U6b and `scripts/**` for verify. Otherwise record them as explicit exceptions to the rule.

---

### M1. How Refused is represented and passed to callers is undefined - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 775, 787, 821-824`
* **Discrepancy Description**: Several parts of the Refused design are unspecified:
  - **Representation.** `classify_path(path, workspace, write) -> Effect` (`src/permissions.rs:595`) has no Refused variant. The plan doesn't say whether Refused becomes a new `Effect`. If it does, that changes `Effect::ALL` and the 11-row table (`permissions.rs:45, 866-880`).
  - **`ReadRoots` storage.** Its type, constructor and storage (a `ToolCtx` field, `App` or a `OnceLock`) aren't given.
  - **Callers without a config.** `ambient::recent_changes` (`src/ambient.rs:12, 22`) and `src/sources.rs:253` have no config to pass in.
  - **A missed caller.** `src/server.rs:1070` also calls `classify_path`, and item 12 doesn't list it.
  - **A hidden second classifier.** The read tools reach `classify_path` only through `read_effect` (`src/tools/fs.rs:16-28`), which already acts as a second classifier for Heron's home. Item 7 doesn't mention it.
  - **Writes.** The verdict for a *write* to a Refused path isn't stated. Today such a write is `Privileged`, which Yolo allows (`fs.rs:30-38`).
* **Rationale & Recommendation**: Give the new signatures, for example `classify_path(path, workspace, roots: &ReadRoots, write)`. Also give:
  - the Refused representation and its deny text;
  - where `ReadRoots` lives and how each of the five callers reaches it;
  - whether `read_effect` is removed or reduced to a call to `classify_path`;
  - the verdict for writes to Refused paths.

### M2. The `OutsideRead` effect is underdefined, and the check order contradicts the verdict table - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 774, 777, 781`
* **Discrepancy Description**:
  - **Missing properties.** The plan gives no `label`, `mutates`, `sends_out`, `high` or `reads_only` for `OutsideRead` (`permissions.rs:59-105`). `reads_only` decides whether calls run in parallel (`src/agent.rs:393`), and parallel asks would collide with Telegram's single pending-question slot (`src/telegram.rs:179`).
  - **Taint in Yolo.** The taint step fires only for `mutates() || sends_out()` (`permissions.rs:244`), so it can't produce the table's "yolo, tainted, not granted -> Ask".
  - **Read-only mode.** `(Mode::ReadOnly, _) => Deny` (`permissions.rs:258`) contradicts the table's read-only OutsideRead -> Ask.
* **Rationale & Recommendation**: Write out the enum diff, every property of the new variant, and the exact match arms of the "tiers" step for OutsideRead, covering both mode and taint.

### M3. Folder-grant scope and broad-grant rejection are not specified mechanically - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Line 778-779`
* **Discrepancy Description**: The grant rules leave these cases open:
  - **Folder-valued tools.** For `list_dir`, `search_files` and `find_files` the target is already a folder, so `<parent>/**` grants more than was asked.
  - **Files directly in home.** Such a file yields `read:$HOME/**`, which is on the rejected list. The plan doesn't say what "always" does then.
  - **Normalisation.** The rejected list is written with `~`, but stored rules are absolute.
  - **Conversation grants.** These compare exact strings (`agent.rs:102-107, 119`), so they won't cover subfolders.
  - **Persisting drops.** Rules come from both `cfg.permissions.allow` and kv `allow_rules` (`app.rs:105-111`), and `run_job` builds its own policy (`app.rs:202`). The plan doesn't say whether dropped rules are written back. If they aren't, the activity-log rows repeat on every start.
  - **Reporting a failed add.** `add_allow` returns `()` (`permissions.rs:166`), so "adding one fails" has no way to report the failure.
  - **The Q2 notice.** The one-time Settings notice has no storage or API field.
* **Rationale & Recommendation**: For each of the four read tools, give:
  - the grant target;
  - the `~` and home normalisation;
  - how conversation grants match, and that `can_always` is false when the grant would be too broad;
  - the new `add_allow` signature;
  - the `store.audit` arguments for a drop, and whether drops are persisted;
  - a `dropped_rules` field on `GET /api/settings`.

### M4. The `search_files` and `find_files` walkers don't filter Refused entries - Severity: Major
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Lines 775, 787`
* **Discrepancy Description**: `search_files` skips only `SensitiveRead` entries (`fs.rs:719`), and `find_files` doesn't filter at all (`fs.rs:744-768`). A search rooted in an Ask-tier folder can therefore list or read files inside Refused folders.
* **Rationale & Recommendation**: Require both walkers to skip Refused paths and secret matches, and add a test for it.

### M5. Tainting the MCP connection refuses every MCP write - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 803-805, 839`
* **Discrepancy Description**: With taint set, the `LocalStateWrite` tools (`remember`, `forget`, tasks and schedules) get Ask (`permissions.rs:244`). MCP turns every Ask into a refusal (`mcp.rs:274`). So "memories written over MCP are unconfirmed" never happens: nothing is written at all. Three related problems:
  - **Unreachable special case.** `save_automation` isn't in `SERVED_TOOLS` (`mcp.rs:232`), so the special case for it can't be reached.
  - **Existing test.** `server_speaks_mcp` (`mcp.rs:306, 322`) expects `remember` to succeed, so it will break.
  - **Ambiguous assertion.** The M-2 test accepts either outcome.
* **Rationale & Recommendation**:
  - Pick one behaviour. Either MCP is read-only for these tools, and line 804 is deleted, or an explicit MCP exception stores `remember` as unconfirmed, placed at a stated point relative to `decide`.
  - Remove the `save_automation` case.
  - List the test changes.

### M6. `recall` can't taint only some of its results, and `recent_activity` is unspecified - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 796-797`
* **Discrepancy Description**:
  - **Per-tool flag.** `untrusted_output()` is a fixed per-tool flag (`src/tools/mod.rs:89-92`), applied to the whole result (`agent.rs:523-527`). `recall` is `untrusted: false` (`src/tools/personal.rs:265`).
  - **Missing file.** `personal.rs` isn't listed.
  - **`recent_activity`.** The plan gives it no file, schema or `Effect`, and doesn't say what it does when ambient observation is off.
* **Rationale & Recommendation**: Choose one: make `recall` always untrusted, add a per-call taint return to `Tool::call`, or split it into `recall` and `recall_unconfirmed`. Add `personal.rs` to the scope and define `recent_activity` fully.

### M7. Session-creating call sites and their `origin` values are not listed - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 814, 893`
* **Discrepancy Description**: Sessions are created at many call sites:
  - `main.rs:304, 651, 653, 671`;
  - `server.rs:331, 449`;
  - `telegram.rs:158`;
  - `app.rs:214, 259, 638`.

  `create_session(title)` (`store.rs:653`) takes no origin. Sub-agent sessions are never stored (`agent.rs:66, 554`), yet `sub_agent` is listed as an origin value. MCP creates no session row.
* **Rationale & Recommendation**: Add a table mapping each call site to its origin value. Give the new `Session::create` and `create_session` signatures.

### M8. The audit v2 hash contradicts v1, and the new fields have no way into `audit()` - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 890-895`
* **Discrepancy Description**:
  - **Hash formula.** The v1 hash length-prefixes each field (`store.rs:12-21`). The v2 hash joins fields with `\x1f`, which can collide when a field contains `\x1f`.
  - **No way in.** `Store::audit(session, tool, args, decision, result)` (`store.rs:1157`) has no `run_id`, `provider` or `cli_version` arguments, and `ToolCtx` doesn't carry them. These callers would need them: `agent.rs:476, 504, 534`, `mcp.rs:267` and `app.rs:407`.
  - **No `run_id` before U3.** The server's `run_id` is a per-process `u64` (`server.rs:453`), and the column type isn't given.
  - **Schema test.** The fingerprint test (`store.rs:1687-1695`) must change.
* **Rationale & Recommendation**:
  - Define v2 as the v1 length-prefixed fields followed by the three new fields, each length-prefixed.
  - Give the new `audit()` signature and where each value comes from.
  - Give the `run_id` type, and what it holds before U3.
  - List the fingerprint test update.

### M9. The pre-migration backup duplicates an existing snapshot - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 896, 1021`
* **Discrepancy Description**: `open_checked` already snapshots before any upgrade, and stops if the snapshot fails (`store.rs:527-540`). The snapshot is named `heron-before-upgrade-from-v{n}-*` and pruned to 3 (`store.rs:234, 370-380, 533`). The plan's `pre-v3-<timestamp>` adds a second copy, and the upgrade test (1021) would look for the wrong name. Line 1021 also lacks an article: "that `pre-v3-*` backup exists".
* **Rationale & Recommendation**: Reuse the existing snapshot, and have `upgrade-test.sh` check `backups/heron-before-upgrade-from-v2-*.db`.

### M10. Config and Cargo changes are missing from U2's file list - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 851, 854-856`; table `1053`
* **Discrepancy Description**:
  - **`config.rs`.** `cli_allow_api_key` is a bool today (`src/config.rs:103, 242`; exposed at `server.rs:227`) and becomes a list. The new `cli_subscription` (per provider), `allow_unattended_subscription` and `cli_topology` have no TOML shape, serde type or compatibility shim. `config.rs` isn't listed.
  - **`Cargo.toml`.** It needs a direct `semver` dependency and a `spike` feature, and it isn't listed either.
* **Rationale & Recommendation**: Add `config.rs` and `Cargo.toml` items with exact fields, types and defaults, plus the feature line. For example, use an untagged enum so `cli_allow_api_key` accepts both `true` and a list. See B8 for scope.

### M11. The subscription hold and the "continue anyway" choice have no implementation - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 854-855`
* **Discrepancy Description**:
  - **The hold.** It is enforced in the schedule, automation, watch, proactive and reflection runners (`app.rs`, `workflow.rs`, `schedule.rs`), and none of those files is listed. "Waiting for you" and "Run now" have no storage or route. The spec's tests `unattended_model_steps_wait_on_subscription_provider` and `unknown_auth_is_held` appear in no test list.
  - **"Continue anyway?"** CUJ 5's prompt needs a route that writes kv `cli_version_ok:<id>`. No such route is planned.
* **Rationale & Recommendation**: Add file items and steps for the hold, a route for accepting a version, and both tests.

### M12. The per-provider registry data is missing - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 849-850, 854, 857-858`
* **Discrepancy Description**:
  - **Registry values.** None of the 16 registry entries (`cli_llm.rs:284-301`) gets `config_dirs` or `supported` values.
  - **API-key variables.** Only claude, codex and gemini have them today (`cli_llm.rs:1221-1231`).
  - **Cline.** Its only argument is `--json` (`cli_llm.rs:225-228`), and the plan gives no argument that turns its tools off.
  - **Version managers.** There is no rule for which nvm, fnm or mise version wins.
* **Rationale & Recommendation**:
  - Add a table with columns for provider id, `config_dirs`, `supported` and the allowed API-key variables. Mark values UNVERIFIED where needed.
  - Give Cline's argv, or keep it experimental.
  - State the order in which version managers are searched.

### M13. The sandbox API, profile structure and non-macOS behaviour are undefined - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 853, 860-879`
* **Discrepancy Description**: `sandbox::wrap(cfg, workspace)` (`sandbox.rs:80`) can't see `read:` grants or Refused roots. The plan leaves out:
  - the new signature and the per-CLI profile function;
  - where in `cli_llm.rs` the child is wrapped;
  - the shell's call site (`src/tools/shell.rs:36`), which isn't in scope;
  - the Bubblewrap branch (`sandbox.rs:85-99`);
  - the Seatbelt skeleton: whether it stays `(allow default)` plus denies, and the metadata reads that path resolution and git need;
  - the exact `DENIED_HINT` text (`sandbox.rs:115`).

  Two parts of the plan clash with the code:
  - `hidden()` deliberately never hides a parent of the working folder (`sandbox.rs:70`), but the plan says Refused always wins.
  - The decision function can't see whether the sandbox is on, yet the plan expects "sandbox-off rows" in the decision table.
* **Rationale & Recommendation**: Specify:
  - the signatures and the spawn site;
  - the profile skeleton and its last-match-wins ordering;
  - the Linux behaviour;
  - what happens when the working folder overlaps a Refused path;
  - where the sandbox-off refusal is enforced;
  - the exact hint string.

### M14. `mcp-serve --refuse-all` can't be built as described, and its spike precondition fails by construction - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 856, 884, 916`
* **Discrepancy Description**:
  - **Command shape.** `McpServe` is a unit variant (`main.rs:90`), so it can't take `--refuse-all` as written.
  - **Startup order.** `Config::load()` (`main.rs:586`) and `App::build` (`main.rs:643`) run before the command is dispatched. Inside the CLI sandbox `~/.heron` is Refused, so loading the config there can fail.
  - **Database dependency.** `mcp::serve` needs a `ToolCtx`, which holds a `Store`, and a `Policy` (`mcp.rs:235`). The plan's path "never opens the database".
  - **Tool list mismatch.** The envelope's tool list includes `delegate` (`agent.rs:174-180`), browser tools and attached MCP tools (`app.rs:78-103`). `ToolRegistry::builtin()` has none of these, so "advertised names equal the envelope's tools" can't hold.
  - **Server name.** The MCP config must name the server `heron`, because the prefix stripped is `mcp__heron__` (`cli_llm.rs:1550`). The plan doesn't say so.
* **Rationale & Recommendation**: Specify:
  - `McpServe { #[arg(long)] refuse_all: bool }`, with an early return before `Config::load`;
  - a separate `mcp::serve_refuse_all(reader, writer, schemas)`;
  - how each turn's real tool schemas reach it, for example a schema file written next to the temporary MCP config;
  - the server name.

### M15. The event bus can't see background sessions, and its internals are unspecified - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 924-927`
* **Discrepancy Description**: The bus lives in `ServerState` (`server.rs:31-41`), but background runs use their own sinks (`app.rs:640` `null_sink()`; `telegram.rs:211`). Also unstated:
  - the bus type, and the threshold at which a lagging subscriber is disconnected;
  - the `run_id` type in events;
  - the `approval_resolved` event that the avatar needs;
  - the `/cancel` response change (today it returns `{"ok": true}`, `server.rs:560`);
  - the `origin` passed to `park_approval` (`store.rs:910`);
  - the `/api` prefix on every route.
* **Rationale & Recommendation**: Move the bus into `App`, adding `app.rs` and `telegram.rs` to the scope, or narrow the spec to HTTP runs. Define the missing events, types and responses.

### M16. The risk-notice gate breaks the Slint app, the web UI, existing tests and the eval runner - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `synthesizer`
* **Affected Scope:** `Lines 927, 947`; table `1056, 1063`
* **Discrepancy Description**: Once `Agent::run_turn` refuses turns until the notice is accepted, every `/api/chat` caller fails:
  - **The Slint app** (`src/bin/desktop/bridge.rs:287`), which the non-goals say not to change.
  - **The web UI** (`assets/index.html`), which has no way to accept the notice.
  - **Existing tests** (`tests/integration.rs:163, 189, 370`).
  - **The eval runner** (`run-core-tasks.sh`), from U3 on, so verify's G7 run would fail.

  The terminal, Telegram and unattended surfaces (`main.rs`, `telegram.rs`, `app.rs`) aren't in item 20's files.
* **Rationale & Recommendation**: State which surfaces are gated. Say how tests and the eval runner pre-accept the notice, for example by seeding the kv key. Add the terminal flow to the scope.

### M17. `--print-endpoint` details are missing, and the Mac app needs routes that don't exist - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 934-935, 957-961`
* **Discrepancy Description**:
  - **The endpoint.** `server::start` returns only a URL (`server.rs:181-192`).
  - **Loopback.** `loopback_only` is derived from `cfg.server.bind` (`server.rs:113-114`), not from the forced bind.
  - **Banner.** It also goes to stdout (`main.rs:679`), mixing with the one JSON line.
  - **Stdin EOF.** No branch watches for it next to the existing `select!` (`main.rs:683-686`).
  - **Routes the Mac app needs that don't exist:**
    - CLI detection, version status and "continue with this version" (`probe_status` at `cli_llm.rs:538` is used only by the terminal);
    - revoking a grant (only `set_tool` removes rules, `server.rs:918-935`);
    - "another core is running": `own_background_work()` returns a boolean that `serve` ignores (`server.rs:172`, `app.rs:670-688`).
* **Rationale & Recommendation**:
  - Give the new return type of `start`, and compute `loopback_only` from the actual bind.
  - Suppress the banner when the flag is set, and add an EOF branch to the `select!`.
  - Add the missing routes, or fields on `/api/state`, with exact shapes. For example, add `"background_owner": false` to the endpoint JSON.

### M18. The G9 count doesn't say which decisions count as executed - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 757-765`
* **Discrepancy Description**:
  - **Decision strings.** These all exist, and the plan doesn't say which count:
    - `allowed`, `approved`, `approved-always`, `approved-for-conversation` (`agent.rs:474-501`);
    - `approved-later` (`app.rs:407`);
    - `allowed-mcp` (`mcp.rs:267`).
  - **Missing file.** Q4's `eval/owner-use.md` isn't created anywhere.
  - **Baseline.** `owner-use.sh` relies on `sessions.origin`, which the baseline doesn't have.
* **Rationale & Recommendation**: Give the exact SQL, including the decision list and the local-time `date()` expression. Add `owner-use.md`, and say how the baseline is handled without `origin`.

### M19. The verification gates are mis-specified - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `style`
* **Affected Scope:** `Lines 1029, 1038, 1040, 1042, 1045, 1071-1072, 1078`
* **Discrepancy Description**:
  - **Mutation base.** If units land on `main` by PR, `testteeth run --base main` on the final branch sees an empty diff.
  - **CI claims.** Line 1072 claims testteeth on each PR, and 1071 lists `cargo fmt --check`. Today's CI has neither step and no clippy step, and item 31 adds none of them.
  - **Clippy.** `clippy -D warnings` has never been enforced, so warnings already in the code can fail U1's check, which isn't U1's fault.
  - **Trial dates.** `<trial start>` and `<trial end>` can't appear literally in a script.
  - **genesis.** The plan gives no command line for `genesis audit`. Its target, `check_tasks.py`, only validates the task file; the grader is the pass-rule code in `run-core-tasks.sh`.
* **Rationale & Recommendation**:
  - Pin the testteeth base, for example the pre-U1 tag `v0.3.1`.
  - Add testteeth, clippy and fmt steps to item 31, or drop the per-PR claim.
  - Add a "clippy clean on main" step before U1.
  - Pass the trial dates as environment variables, and skip the step when they are unset.
  - Give the exact genesis command, and point it at the pass-rule grader.

### M20. U4's Xcode project creation and test dependencies are left open - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Lines 951-970, 1013`
* **Discrepancy Description**: The plan doesn't say:
  - **Project creation.** How `HeronMac.xcodeproj` is produced without the Xcode GUI: XcodeGen, Tuist or a hand-written pbxproj.
  - **Snapshot library.** XCTest has none built in.
  - **Bundle id and product name.** The product must be named `Heron` to match line 1013.
  - **Deployment target.** macOS 14.
  - **Universal build.** The NFR requires one, but the flags (`ARCHS`, `ONLY_ACTIVE_ARCH`) aren't given.
* **Rationale & Recommendation**: Pin all of these.

### M21. The `build-app.sh` and signing steps are incomplete - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `style`
* **Affected Scope:** `Lines 1008-1013`
* **Discrepancy Description**:
  - **Slint build.** `build-app.sh:9` still builds `--features desktop`, which builds Slint. The plan doesn't say to drop it.
  - **Bundle assembly.** The plan doesn't say whether the Xcode `Heron.app` becomes the whole base bundle or only its executable is copied. Line 26 overwrites `Info.plist` from `packaging/macos/Info.plist`, which sets `LSMinimumSystemVersion 11.0` and `app.heron.desktop`.
  - **`--unsigned`.** "Does the same with no signing" (1010) would drop the ad-hoc `codesign --sign -` at line 30, which Apple silicon needs. The phrase also leaves the steps implied.
  - **CI signing.** There is no step for importing the Developer ID certificate into a CI keychain. Only the ASC secrets are named (`release.yml:65-81`).
  - **Notarization and entitlements.** The plan doesn't say whether the app or the DMG is notarized and stapled, or where the entitlements file lives.
  - **Network entitlement.** "`com.apple.security.network.client` only for loopback" (1012) is ambiguous, and that entitlement only has an effect inside the App Sandbox.
* **Rationale & Recommendation**: Spell out:
  - the bundle assembly order and the `Info.plist` source;
  - the `--unsigned` steps, including whether ad-hoc signing stays;
  - the CI steps and secrets for importing the certificate;
  - the notarization target and the entitlements file path.

### M22. The upgrade fixture has no source, and the release label contradicts the plan - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `style`
* **Affected Scope:** `Lines 1016-1023, 1028`
* **Discrepancy Description**:
  - **Fixture.** No file in the list holds the "fixture v0.3.1 `~/.heron`". The only fixture in the repo is `tests/fixtures/schema-v0.1.0.sql`.
  - **Upgrade test steps.** `HERON_HOME` isolation (`config.rs:293`) isn't addressed, and the plan doesn't say which command "runs the new core once". "Rehearses `heron restore`" (1023) gives no pass condition.
  - **Release label.** `release.yml:164` publishes the release as "(beta)" with `--prerelease`, and checks the version against the tag and the CHANGELOG (`release.yml:50-53`). No unit changes the label to "Research preview" or bumps the version.
* **Rationale & Recommendation**: Name the fixture's path and the script that generates it. Give the exact commands and pass conditions for the upgrade test and the restore rehearsal. Add the label change and the version bump to item 31.

### M23. U6 and U6b targets are incomplete, and the file sections don't match the table - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `style`
* **Affected Scope:** `Lines 979-1029`; table `1059-1062`
* **Discrepancy Description**:
  - **Unlisted files.** `docs/engineering/SECURITY_THREAT_MODEL.md` and `REMEDIATION_MATRIX.md` contain neither C35-8 nor R-1, and they must be edited to "link back". Neither is a target file.
  - **Item 26.** It lacks the `Version: <integer>` first-line rule and the format of the owner's signature.
  - **Item 27.** It lacks the mode-table change (`README.md:121-126`) that the U6b row requires.
  - **Headings and table disagree:**
    - `README.md` sits under the U6 heading, but the table gives it to U6b.
    - Item 31 sits under U7, but U7b owns `.github/**`.
    - U2w and U7b have no file headings at all.
    - The table's `verify` unit is called "U8" in the headings.
* **Rationale & Recommendation**: Add the missing files and steps. Give U2w, U6b and U7b their own headings, move items 27 and 31 under them, and use one name for the verify unit.

### M24. Two checks can pass while running zero tests - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `synthesizer`
* **Affected Scope:** `Line 907`; table `1053, 1056`
* **Discrepancy Description**:
  - **U2.** Its check filters on `fake_`, but item 18 never says the fake-CLI tests carry that prefix.
  - **U3.** Its `--test integration api_` filter matches four of its seven new tests; the other three don't start with `api_`.

  A test with a mismatched name silently drops out of the check.
* **Rationale & Recommendation**: Name the fake tests in item 18, and make each check fail when nothing runs, for example by checking the `running N tests` count.

### M25. The spike can't select the hybrid arm, and its report wording reopens its scope - Severity: Major
* **Reviewer Lens:** `style`, `implementation-readiness`
* **Affected Scope:** `Lines 750, 914-916`
* **Discrepancy Description**:
  - **No way to run hybrid.** `run-core-tasks.sh` has no topology or feature-build option, so U2s can't run the hybrid arm.
  - **Scope wording.** Line 914 says "one decision", but 916 says the report "records each chosen default", which looks left over from the wider spike that stage 8 cut.
  - **Partial rule.** Line 914 restates the decision rule without its token condition (spec line 408: no more than 5% more tokens per task).
* **Rationale & Recommendation**: Add `--topology envelope|hybrid` to the runner (see B8). Change 916 to "records the chosen topology and why". Link the decision rule instead of restating part of it.

### M26. The secret-pattern additions are not exact - Severity: Major
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Line 776`
* **Discrepancy Description**: "Gains the M-4 paths" leaves the regex to the implementer. It doesn't say which shell-history files count, or what the exact `Login Data` pattern is. `credentials.toml` isn't matched today: the pattern is `credentials(\.json)?$` (`permissions.rs:541`).
* **Rationale & Recommendation**: Give the exact new pattern.

### M27. Existing tests that will break aren't listed, and a test name differs from the spec - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 826-841`
* **Discrepancy Description**:
  - **Breaking tests:**
    - Removing `decide()` and the boolean taint argument touches more than 40 test calls (`permissions.rs:837-1018`), plus `agent.rs:791` and `app.rs:986`.
    - `promises_that_hold_whatever_the_settings` asserts `4*11*2*5*3` combinations (`permissions.rs:946`).
    - `path_sandbox` expects outside paths to be `LocalRead` and `~/.heron/secrets.json` to be `SensitiveRead` (`permissions.rs:1030, 1037`).
    - `fs.rs:1224-1238` covers Heron's home.
  - **Name mismatch.** The M-4 test name ends `_deny_or_ask`, but the spec's findings table (line 52) says `..._ask`, and G3 checks against that table.
  - **Unnamed tests.** The M-1, M-2 and M-6 to M-8 regression tests have no names.
* **Rationale & Recommendation**: List the existing tests to rewrite, name every regression test, and make the names match the findings table.

### M28. The Telegram forward fix is underspecified - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** `Lines 812, 819`
* **Discrepancy Description**:
  - **Forwarded flag.** `handle_message` receives only `text: String` (`telegram.rs:163, 270-273`), so `forward_origin` is lost before it gets there.
  - **Taint steps.** The plan doesn't give the wrapper (`wrap_untrusted`?) or the taint steps (`session.tainted` and kv `tainted:<id>`).
  - **Proactive sessions.** They also need `act_on_suggestion` to taint the new session (`app.rs:638`).
* **Rationale & Recommendation**: Give the parsing and taint steps exactly, and re-check them against `telegram.rs` after #35 lands.

---

### N1. The same commands are written differently - Severity: Nit
* **Reviewer Lens:** `style`, `implementation-readiness`
* **Affected Scope:** `Lines 1040, 1069, 1071, 1072, 1075`
* **Discrepancy Description**:
  - `-q` is present at 1040 and missing at 1072.
  - Line 1075 lacks `--locked`, and uses `--ignored` where the unit check uses `--include-ignored`.
  - The `xcodebuild` at 1069 lacks `-project`.
  - `cargo fmt --check` (1071) is missing from `verify-v1.sh`.
* **Rationale & Recommendation**: Use one form of each command.

### N2. Rules are stated twice or used before they are defined - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Lines 739, 755, 1041, 1045, 1078`
* **Discrepancy Description**:
  - The G7 rule is spelled out in full at both 755 and 1041.
  - `genesis audit` is stated at both 1045 and 1078.
  - "The 26 of 30" appears at 739, before the rule is defined.
  - "Report-only ... Its result is reported" repeats itself.
* **Rationale & Recommendation**: Define each rule once and refer back to it. At 739, write "doesn't count toward the G7 gate".

### N3. An absolute guarantee - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Line 824`
* **Discrepancy Description**: "No database, key or CLI credential is ever indexed or observed" overclaims for a check based on patterns.
* **Rationale & Recommendation**: Write "Refused paths and secret-pattern matches are not indexed or observed."

### N4. An unlinked reference and an informal phrase - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Lines 956, 970`
* **Discrepancy Description**: "The diagram" (956) has no link. "Pay off" (970) is informal, and the claim can't be checked.
* **Rationale & Recommendation**: Link the diagram's section or delete the sentence. Give the reason for excluding other screens plainly.

### N5. "Only for loopback" can be read two ways - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Line 1012`
* **Discrepancy Description**: It can mean "this one entitlement, which loopback needs", or "the entitlement is limited to loopback". See also M21.
* **Rationale & Recommendation**: Write "The SwiftUI app gets one entitlement, `com.apple.security.network.client`, which it needs to reach the core on 127.0.0.1."

### N6. U5's dependency is listed as a range - Severity: Nit
* **Reviewer Lens:** `style`
* **Affected Scope:** `Line 1063`
* **Discrepancy Description**: It is unclear whether "U0-U7b" includes U5 when U5 is deferred. See B7.
* **Rationale & Recommendation**: List verify's dependencies explicitly (see N7).

### N7. The scheduling text is unclear - Severity: Nit
* **Reviewer Lens:** `style`, `synthesizer`
* **Affected Scope:** `Lines 1063, 1065`
* **Discrepancy Description**: "U0-U7b" is a range over ids that aren't in sequence (U2s, U2w, U6b, U7b). "U4 with U2w and U6" can be read as all three running in parallel, but U6 depends on U2w.
* **Rationale & Recommendation**: List verify's dependencies by id. Write "U4 runs in parallel with U2w, and later with U6".

### N8. The CLI run-folder name is undefined - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Line 852`
* **Discrepancy Description**: `<run>` in `$TMPDIR/heron-cli-<run>` isn't defined, and the existing test at `cli_llm.rs:2363` depends on `~/.heron/tmp`.
* **Rationale & Recommendation**: Define `<run>` and list the test update.

### N9. The EVE config and baseline are unspecified - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Lines 744, 765`
* **Discrepancy Description**: There is no EVE config schema, and no run count, provider or pinned CLI version. The baseline's provider and run count aren't stated either.
* **Rationale & Recommendation**: Give the YAML and the baseline parameters, so later runs compare like with like.

### N10. The hybrid topology's Claude flags aren't given - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** `Line 856`
* **Discrepancy Description**: The plan doesn't say whether `--strict-mcp-config` is used, or how `--tools ''` interacts with MCP tools. Neither can be checked from the repo.
* **Rationale & Recommendation**: Give the exact argv.

### N11. Three checks are fragile - Severity: Nit
* **Reviewer Lens:** `synthesizer`
* **Affected Scope:** Table `1051, 1054, 1055`
* **Discrepancy Description**:
  - **U0.** With `$TMPDIR` unset, as in stripped environments, the check writes to `/u0`.
  - **U2s.** The check passes on any file containing a `Chosen default:` line, so it proves nothing about the 50 paired runs.
  - **U2w.** The grep `feature = "spike"` misses `feature="spike"` written without spaces.
* **Rationale & Recommendation**:
  - Use `"${TMPDIR:-/tmp}/u0"`.
  - Have U2s also check that the run counts per arm are present.
  - Use the regex `feature *= *"spike"`.

### N12. Preconditions aren't met in this tree, and line references will drift - Severity: Nit
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`
* **Affected Scope:** `Line 719` and the line references throughout
* **Discrepancy Description**: PRs #26 and #35 aren't merged here. `upcoming_events` doesn't exist, but `src/telegram.rs` does. There are two small citation differences: `mcp::serve` spans `mcp.rs:234-289`, and the registry spans `cli_llm.rs:284-301`.
* **Rationale & Recommendation**: Re-check the cited line numbers after both PRs merge and before U1 starts.

---

## Dismissed or not verifiable

- The style lens considered three items and dropped them:
  - "exit 0 SOUND" (1078) and "friend" (1000) are both defined in the document.
  - The `cli_canary` invocations at 1053 and 1075 differ on purpose.
- Some findings depend on external tool behaviour that can't be checked from the repo:
  - Seatbelt semantics;
  - the Telegram Bot API's `forward_origin` field;
  - whether `bwrap` works on GitHub runners;
  - Apple's entitlement behaviour;
  - Claude CLI flags.

  Where these appear above, they are reported as gaps in the plan, not as confirmed defects.
