# Goldfish Review Report: Stage 6, Comprehension Test

**Target:** `/Users/inan/Documents/PERSONAL/HERON-Project/.claude/worktrees/ultrafix-design/docs/designs/heron-harness-safety.md` (FeatureRange 1-619, DetailedRange 620-881)
**Reviewer:** `egm-comprehension` (one instance), standard `re-implementation-standard.md`
**Verdict:** does not pass the re-implementation standard. 2 Blockers, 24 Majors, 9 Nits.
**Sources the reviewer checked:** `src/store.rs`, `src/agent.rs`, `src/app.rs`, `src/permissions.rs`, `src/sandbox.rs`, `src/tools/shell.rs`, `src/server.rs`, `src/util.rs`; anchor-harness `repeats.rs`, `proxy.rs`, `sandbox.rs`; harness2 working copy (may differ from the cited revision).

## Prioritized Findings (Blockers -> Major -> Nits)

### CO-B1 Rehash of Heron's home reports a change after every run - Severity: Blocker
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.1 line 377; §3.3 lines 436-445; module table line 246
* **Discrepancy Description:** Line 377 protects `cfg.home` and `~/.heron` "so the rehash covers config.toml and heron.db". §3.3 hashes each file in a protected folder and reports any change. Heron writes `heron.db` and its WAL in every run (journal rows, audit rows, messages), and lease files plus `background.lock` live in the home. G5 fires on every journaled run.
* **Rationale & Recommendation:** Say which home files are snapshotted, whether `heron.db` is compared at all, and exclude `-wal`, `-shm`, `journal/*.lock` and `background.lock`.

### CO-B2 Recovery atomicity contradicts itself - Severity: Blocker
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §1.4 lines 308-313; file 1 lines 651-652; file 8 lines 770-771
* **Discrepancy Description:** §1.4 says one transaction marks rows interrupted and creates their approvals, so no row is left without one. File 1 splits `recover -> Vec<Interrupted>` from `ask_again`. File 8 calls both in `own_background_work`, but only `recover` on each `scheduler_loop` tick, so tick-recovered rows become `interrupted` with no approval. `park_approval` takes the store mutex itself (`src/store.rs:1060`), so it cannot run inside a `with_conn` transaction.
* **Rationale & Recommendation:** Decide whether `recover` creates approvals, whether it inserts into `approvals` with raw SQL inside the transaction, and make the tick call `ask_again` too.

### CO-M1 Loop-guard notice text differs between CUJs - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** CUJ 3 line 172; CUJ 4 line 179
* **Discrepancy Description:** CUJ 3 says "stopped: the same 2 calls"; the port emits "stuck:" (`repeats.rs:76`), and CUJ 4 uses "stuck:".
* **Rationale & Recommendation:** Pick one text for tests to assert.

### CO-M2 The nudge changes meaning - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §2 line 356; A4 line 527
* **Discrepancy Description:** Today's nudge keys on `name:arguments` only (`src/agent.rs:559-561`); `count()` includes output, so a repeated call with a different result is no longer nudged. The doc does not say whether `observe` sees raw output or the `wrap_untrusted` text (`src/agent.rs:699`, `src/util.rs:89`).
* **Rationale & Recommendation:** State the behaviour change and which output form is fingerprinted.

### CO-M3 A sub-agent's stop notice is lost - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §2 line 357
* **Discrepancy Description:** The delegate child's sink drops `Event::Notice` (`src/agent.rs:729-734`), so a stopped sub-agent's notice never reaches the app.
* **Rationale & Recommendation:** Say whether the stop is forwarded to the parent or surfaced otherwise.

### CO-M4 `run_id` storage and overwrite rules undefined - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §1.2 lines 286-292; file 8 line 773
* **Discrepancy Description:** `run_turn` takes `&self` (`src/agent.rs:468`), so the field needs interior mutability, and two turns on one `Agent` overwrite each other. "Set in `run_turn` at depth 0" overwrites the `job:<id>` set for job agents. Heron already has an events run id (`crate::events::new_run_id()`, `src/app.rs:1565`), and scheduled jobs use it rather than `job_runs.id`.
* **Rationale & Recommendation:** Define storage, whether a preset `job:` id survives, and the relation to the events run id.

### CO-M5 Timeout and cancellation inside `guarded` undefined - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §1.3 lines 296-299
* **Discrepancy Description:** `tool.call` runs inside `tokio::time::timeout` (`src/agent.rs:685`; `src/app.rs:1804`), and turns can be cancelled. If `guarded`'s future is dropped, the intent stays open under a live lease, possibly for days. How `done` vs `failed` is decided is undefined; the shell timeout returns `Ok("[timed out ...]")` (`src/tools/shell.rs:107`).
* **Rationale & Recommendation:** State nesting order with the timeout, what a drop writes (for example an `abandoned` outcome via a drop guard), and the done/failed rule.

### CO-M6 State machine after `asked`, and retention, undefined - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §1.2 lines 280-285; Security line 576; test line 667
* **Discrepancy Description:** No transition after Run again or Dismiss. Unclear whether `asked`/`interrupted` count as "finished" for the 30-day prune, and when `args` is nulled for interrupted rows.
* **Rationale & Recommendation:** Give the full state machine and which states the prune treats as open.

### CO-M7 Lease ordering and missing lease file - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §1.4 lines 307-310
* **Discrepancy Description:** If the intent commits before the lease file exists, recovery's `try_lock` creates and locks a fresh file and judges a live process dead. Behaviour of `is_dead` on a missing file, clean-exit removal and cleanup of lease files with no intents are undefined.
* **Rationale & Recommendation:** Require lease before first insert; define missing-file handling and cleanup.

### CO-M8 "Exactly one approval" vs `park_approval` dedupe - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** G1 line 49; §1.5 lines 318-322
* **Discrepancy Description:** `park_approval` returns an existing `new` row with the same tool, args and origin (`src/store.rs:1061-1064`), so two identical interrupted calls become one approval.
* **Rationale & Recommendation:** State whether this is intended and which effects rows get its `approval_id`.

### CO-M9 Ask-again summary text source - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** CUJ 1 line 153; §1.5 line 320
* **Discrepancy Description:** CUJ 1 shows "Run `git commit -m …` was interrupted"; the shell summary is the raw command (`src/tools/shell.rs:11-13`). Where "Run" comes from and whether the summary is truncated is unstated.
* **Rationale & Recommendation:** Define the template exactly.

### CO-M10 Action-entry interface has undefined parts - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §1.5 lines 327-341
* **Discrepancy Description:** `label` has no column in `effects`; `ActionIntent.session_id` type and the no-session case; mapping of `rerun`'s `Ok`/`Err` to approval status; whether `rerun` runs inside `guarded`; when and on which table "status `failed`" is set for an unknown handler; whether the registry is a process-global static.
* **Rationale & Recommendation:** Define each, since this is the interface Pre-5 depends on.

### CO-M11 How `is_protected` finds repositories for the file tools - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.1 lines 369-379
* **Discrepancy Description:** `classify_path` is synchronous and per path. Unclear whether it probes for `.git` at call time and follows `gitdir:` to a `commondir` outside the workspace. If it does, a repo the agent `git init`s is protected for `write_file` but not for the shell (whose rules come from the cached scan). Case sensitivity of matching is unstated.
* **Rationale & Recommendation:** Specify the lookup, and whether the file-tool/shell split is intended.

### CO-M12 No channel for the refusal reason - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.1 line 379; file 9 line 783; CUJ 5 line 187
* **Discrepancy Description:** `classify_path` returns only an `Effect` (`src/permissions.rs:1117`). The CUJ 5 reason text has no path to the deny message.
* **Rationale & Recommendation:** Name the carrier (new variant payload, side return, or deny-message lookup).

### CO-M13 Watch key and lifecycle - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.3 lines 436-442; §3.1 line 387; file 8 line 772
* **Discrepancy Description:** Keying by session id lets a chat turn and a `run_parked` collide. Line 387 says `run_parked` scans afresh; lines 442 and 772 give it its own watch. Automations call tools via `call_tool` without `run_turn` (`src/app.rs:1640-1641`), so nothing calls `watch_end` for them (G5 gap, leaked map entry). `run_parked` has no sink for the notice.
* **Rationale & Recommendation:** Key by run id; define the automation path and the `run_parked` notice route.

### CO-M14 Diff precedence for new repositories - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.3 lines 443-445
* **Discrepancy Description:** Rule 2 reports any new protected file; a new repo's `.git/config` and hooks are new protected files. Rule 3 exempts samples and `NEW_REPO_CONFIG_KEYS`. Precedence and whether deletion counts are undefined.
* **Rationale & Recommendation:** State precedence and the deletion rule.

### CO-M15 Unstated breakage from protecting `.git/config` and dot-folders - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.1 lines 370-371; §3.2 lines 425-432
* **Discrepancy Description:** Sandboxed `git remote add`, `git branch -u`, `git push -u`, `git checkout -b x origin/x` write `.git/config`. Per-file hashing of `.claude/`, `.idea/`, `.vscode/` has no byte cap; `.claude/worktrees/` can hold whole worktrees.
* **Rationale & Recommendation:** Add a "What breaks" list for protected paths and a hashing cap.

### CO-M16 Egress settings semantics undefined - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.2 lines 414-419
* **Discrepancy Description:** Whether `egress_allow` defaults to `DEFAULT_ALLOW`, whether a user list replaces or extends it, what an invalid `egress` value does, and whether a failed proxy start fails closed.
* **Rationale & Recommendation:** Specify; failing closed is the conservative default.

### CO-M17 Nobody writes kv `server:port` - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** OQ4 line 115; file 11
* **Discrepancy Description:** No code writes that key today, and file 11 does not list it. Stale values and processes without a server are unspecified.
* **Rationale & Recommendation:** Add the write to file 11 and define staleness handling.

### CO-M18 `NoBaseline` transition and toolchain state undefined - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.4 lines 462, 476-479; test line 734
* **Discrepancy Description:** After a first-use baseline, the "after" rule ("was Trusted, now Changed") never fires for that first command. Unclear whether `NoBaseline` refuses anything, whether `Changed` is derived per check or read from kv `toolchain:changed`, and when that key is cleared. The `touch_without_content_change` test refreshes stat data, contradicting "only `trust_current` writes the baseline". `ChangedFile`, `since` and a deleted toolchain are undefined.
* **Rationale & Recommendation:** Give the state transitions and which writer may refresh stat data.

### CO-M19 Toolchain baseline in a disposable home - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.4 line 481; §1.5 line 343
* **Discrepancy Description:** The repair child's `toolchain::check` reads an empty kv and re-baselines on first use, so the "refusal and Seatbelt deny cover repair runs" claim fails.
* **Rationale & Recommendation:** Name which store the baseline comes from.

### CO-M20 G8 vs toolchain refusal on Linux and Windows - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** G8 line 58; line 67; file 12 line 823; line 466
* **Discrepancy Description:** Line 823 applies the toolchain refusal on Linux and Windows. Windows has no inode/ctime in the same sense, so the stat-first argument does not carry.
* **Rationale & Recommendation:** Reword G8 or limit to macOS; name the fields used per platform.

### CO-M21 "First word of any part" undefined - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** §3.4 line 477
* **Discrepancy Description:** Separators (`;`, `&&`, `||`, `|`, newline, `$(`, backticks) and prefixes (`VAR=x cargo`, `env cargo`, `xargs cargo`) are undefined.
* **Rationale & Recommendation:** Define, and name any existing splitter to reuse.

### CO-M22 Pre-S1 stub vs schema step signature and fingerprint test - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** Pre-S1 lines 626-628; file 1 line 639
* **Discrepancy Description:** Modules are "empty", yet a `journal::sql::step` stub is required. `Step` is `fn(&Connection) -> Result<()>` (`src/store.rs:187`), but file 1 gives `step(tx: &Transaction)`. Adding `to_v4` breaks `the_schema_cannot_change_without_a_new_version`, pinned at `(3, "878cc8a2270e5e69")` (`src/store.rs:2073`); `--no-run` will not catch it, and W1's table changes the fingerprint again. No unit owns that test.
* **Rationale & Recommendation:** Fix the signature, and assign the fingerprint update to W1 or W6.

### CO-M23 Build tooling undefined in the doc - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** lines 126, 622, 864-867; Work units table
* **Discrepancy Description:** skein, `skein run`, skein supervisor, nodes, `--verify-timeout` and the model-role values are neither defined nor linked.
* **Rationale & Recommendation:** Link the skein rules doc or define the terms briefly. (Synthesizer note: this is a project-wide convention; a one-line link suffices.)

### CO-M24 W1 depends on W6's `reopen_approval` - Severity: Major
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** file 1 test line 663; file 6 line 746; Work units W1, W6
* **Discrepancy Description:** W1's `interrupted_parked_call_reopens_its_own_approval` needs `reopen_approval`, which W6 adds to `src/store.rs`; W1's scope is `src/journal/**`. W6's check does not include the schema fingerprint test.
* **Rationale & Recommendation:** Move `reopen_approval` into Pre-S1, or have W1 reopen through `with_conn` SQL and drop W6's copy.

### CO-N1 "Read-only mode refuses exactly this set" omits Forbidden - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** line 261
* **Discrepancy Description:** `mutates()` also includes `Forbidden` (`src/permissions.rs:98`).
* **Rationale & Recommendation:** Correct the sentence.

### CO-N2 "`delegate` is not a tool" - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** line 263
* **Discrepancy Description:** The model calls it as one (`src/agent.rs:621`).
* **Rationale & Recommendation:** Say it is intercepted before the gate and never journaled.

### CO-N3 Signatures do not match between sections - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** lines 379 vs 705; lines 381 vs 706
* **Discrepancy Description:** `is_protected` gains `heron_homes`; `seatbelt_rules` takes `repos` in one place and `&Scan -> (String, Vec<PathBuf>)` in another.
* **Rationale & Recommendation:** Use one signature.

### CO-N4 `async fn` in a dyn trait needs `#[async_trait]` - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** lines 332-335
* **Discrepancy Description:** `Arc<dyn Resumer>` with `async fn` needs `#[async_trait]` (already a dependency, `Cargo.toml:24`).
* **Rationale & Recommendation:** Say so.

### CO-N5 Ported `decide` also takes `caps` and denies IP literals - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** line 395
* **Discrepancy Description:** anchor-harness `proxy.rs:79-98` takes `caps` (`egress_allow_max`) and denies IP literals.
* **Rationale & Recommendation:** State whether `caps` is dropped and IP literals stay denied.

### CO-N6 Blocked-note text only for one reason - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** CUJ 7 line 205
* **Discrepancy Description:** No text for not-public-address, SNI mismatch, wrong port, or non-CONNECT.
* **Rationale & Recommendation:** Define each.

### CO-N7 Recovery tick placement and interval - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** line 308
* **Discrepancy Description:** The tick must run only inside the `own_background_work()` branch (`src/app.rs:2134`). The tick is 20 s, not "within a minute".
* **Rationale & Recommendation:** State both.

### CO-N8 Undefined or unlinked references - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** lines 27, 114, 426, 603, 608, 856
* **Discrepancy Description:** "t10's failed run", "the Axiom proxy", `00-synthesis.md` and the research folder (no path), PR #73, genesis claims; Heron terms used without definition (taint, parked approval, walks, inbox, kv, `Origin` string form, Refused vs Forbidden, "never" rules).
* **Rationale & Recommendation:** Add paths and a short glossary or links.

### CO-N9 Supervisor-run checks not marked in the table - Severity: Nit
* **Reviewer Lens:** `comprehension`
* **Affected Scope:** line 864; Work units table
* **Discrepancy Description:** The run notes say W3, W6, W8 and verify are supervisor-run; the table does not say so.
* **Rationale & Recommendation:** Mark it in the table.
