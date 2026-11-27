# Goldfish Review Report: Stage 9, Implementation Readiness

**Target:** `/Users/inan/Documents/PERSONAL/HERON-Project/.claude/worktrees/ultrafix-design/docs/designs/heron-harness-safety.md` (DetailedRange, lines 620-881, including the Work units table)
**Reviewers:** `egm-implementation-readiness` (implementation-readiness-standard), `egm-codebase-details` (implementation-readiness-standard), `egm-style` (persona only), one instance each.
**Counts as reported:** implementation-readiness 4 Blockers / 12 Majors / 5 Nits; codebase-details 1 / 21 / 12; style 1 / 14 / 16. Overlaps inside this stage are merged below.

## Work units check (answers to the brief, plus synthesizer checks)

| Row | Single-dir scope | Check runnable | Fails before the unit is built | Model role | Notes |
|---|---|---|---|---|---|
| W1 | yes, `src/journal/**` | yes | **no**: Pre-S1 leaves an empty module, so `-- journal::` matches 0 tests and exits 0 | yes | Needs W6's `reopen_approval` (IR-B3) |
| W2 | yes | yes | **no** (same) | yes | |
| W3 | yes | supervisor only (binds ports) | **no** (same) | yes | Node gets no test feedback (IR-M1) |
| W4 | yes | yes | **no** (same) | yes | |
| W5 | yes | yes | **no** (same) | yes | |
| W6 | yes, `src/**` (contains W1-W5; serialized by depends-on) | supervisor only | partly: filter runs only tests that already exist, does not prove the ~30 new tests exist | yes | Fails for an unplanned reason: schema fingerprint test (IR-B2) |
| W7 | yes, `docs/**` (contains verify's file) | yes | yes: neither grep string exists today (synthesizer checked both files: 0 matches) | yes | Weak: one phrase per file for five required items |
| W8 | yes | supervisor only (`swift`) | unverified whether `swift test --filter` exits 0 on zero matches | yes | Three `CoreClient` conformers, not one (CD-M18) |
| verify | **no**: a single file, not a directory glob | supervisor only | yes | yes | Cannot write the tests that line 855 says kill surviving mutants (IR-B4) |

Model roles: every row's value matches a line in the owner's model-role table (`hardest tasks: opus high`, `feature, refactoring: sonnet high`), checked by the synthesizer.

## Prioritized Findings (Blockers -> Major -> Nits)

### IR-B1 Most checks pass before the unit is built - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness` (also `codebase-details` N9)
* **Affected Scope:** Work units lines 873-878, 880; Pre-S1 line 626
* **Discrepancy Description:** Pre-S1 creates empty modules, so `cargo test --locked --lib -- <module>::` matches zero tests and exits 0. No existing test path has these prefixes. W6's filter does not prove its new tests exist. W8's zero-match behaviour is unverified.
* **Rationale & Recommendation:** Start each check by asserting every named test is listed, then run them. Line 866 already uses the idiom: `L=$(cargo test --locked --lib -- --list) && for t in journal::tests::args_hash_ignores_key_order …; do grep -q "$t: test" <<<"$L" || exit 1; done && cargo test --locked --lib -- journal::`. Same for W6 and W8.

### IR-B2 Pre-S1 leaves store tests failing; its check hides it; step signature does not fit - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `codebase-details` (M3, M4), `style` (M1); also Stage 6 CO-M22
* **Affected Scope:** Pre-S1 lines 626-630; file 1 line 639; file 6 line 748
* **Discrepancy Description:** Adding `to_v4` makes `SCHEMA_VERSION` 4. `the_schema_cannot_change_without_a_new_version` pins `(3, "878cc8a2270e5e69")` (`src/store.rs:2073`) and `to_v3_adds_columns_and_keeps_rows` asserts version 3 (`src/store.rs:2040`); both fail. Pre-S1's `--no-run` does not notice. W1 changes the fingerprint again but cannot edit `src/store.rs`; no unit owns the update and there is no `to_v4` upgrade test. `type Step = fn(&Connection) -> Result<()>` (`src/store.rs:187`) does not accept `step(tx: &Transaction)`. "Each module as an empty `mod.rs`" conflicts with the `journal::sql::step` stub. (Synthesizer verified lines 187, 2040 and 2073.)
* **Rationale & Recommendation:** Either W6 adds `to_v4` and re-pins the fingerprint (W1's tests create the table through `with_conn`), or Pre-S1 adds the DDL and re-pins. Signature `pub fn step(conn: &Connection) -> Result<()>`. List the stub files. Pre-S1's check runs `cargo test --locked`.

### IR-B3 W1 needs `reopen_approval`, which W6 writes - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `codebase-details` (M1), `style` (M8); also Stage 6 CO-M24
* **Affected Scope:** lines 652, 663, 746
* **Rationale & Recommendation:** Add `reopen_approval(id, summary, detail)` to Pre-S1 with its SQL, or have `journal::sql` do the update via `with_conn` and drop W6's copy. State whether `ask_again` calls `park_approval` or returns its arguments.

### IR-B4 Verify's scope is a file, and it cannot kill surviving mutants - Severity: Blocker
* **Reviewer Lens:** `implementation-readiness`, `style` (N2)
* **Affected Scope:** lines 855, 881
* **Discrepancy Description:** The scope is not a directory glob, and line 855 requires new tests for surviving mutants, which a docs-file scope cannot write. A testteeth score under 70 cannot be fixed inside the graph.
* **Rationale & Recommendation:** Give verify `src/**` (it runs last), or define a fix-node rule (`W-fix-<n>`, `src/**`) as `heron-v1.md:1528` does.

### CD-B1 Default `~/.heron` protection blocks Harness2's repair - Severity: Blocker
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** OQ3 line 111; line 377; test line 715
* **Discrepancy Description:** Same as Stage 5 ZO-1 / Stage 7 DC-B2 / Stage 8 Z8-2. Synthesizer verified `heron-harness2.md:420-421` (working copy) and `ReadRoots::build` refusing only `heron_home` (`src/permissions.rs:949`). The default home gets only `SEATBELT_SECRET_FOLDERS` and the `heron.db`/`secrets.json` patterns (`src/sandbox.rs:95`).
* **Rationale & Recommendation:** Protect only `config.toml`, `heron.db*`, `secrets.json` under the default home; correct line 377.

### SY9-B1 Who creates the ask-again approval is ambiguous - Severity: Blocker (style); Major (IR-M3, CD-M2)
* **Reviewer Lens:** `style`, `implementation-readiness`, `codebase-details`; also Stage 6 CO-B2
* **Affected Scope:** lines 232, 308-313, 651-652, 770-771
* **Discrepancy Description:** §1.4 says `recover` creates approvals in one transaction; file 8 calls `recover` then `ask_again`; the tick calls only `recover`. `park_approval` locks the non-reentrant `std::sync::Mutex` (`src/store.rs:1059-1060`, synthesizer verified), so calling it inside `with_conn` deadlocks. A crash between the two leaves orphaned `interrupted` rows that the next `recover` (which lists `intent` rows) never re-asks. Breaks G1.
* **Rationale & Recommendation:** Prefer Stage 8's simplification: `recover` also re-asks any `interrupted` row with NULL `approval_id` (idempotent second pass), and the tick does the same. State that `with_conn` closures must not call `Store` methods.

### IR-M1 "The supervisor runs them" is undefined; W3 and W6 build without test feedback - Severity: Major
* **Reviewer Lens:** `implementation-readiness`; also Stage 6 CO-M23, CO-N9
* **Affected Scope:** lines 697, 824, 864-867
* **Discrepancy Description:** No backend, command or failure loop is named. The mechanism exists elsewhere in the project (`heron-v1.md:822-825`: skein runs each completion command itself, outside the agent's allowlist) but is not cited. Unverified: that node sandboxes block loopback bind and `sandbox-exec`, and that `--verify-timeout` exists.
* **Rationale & Recommendation:** Cite the mechanism and backend; say what happens on failure. Split each check into node-runnable tests and supervisor-only tests (for example an `egress::proxy::net` submodule run with `--skip` in the node). Mark supervisor-only checks in the table.

### IR-M4 / CD-M11 `guarded` has no Heron home for the lease; core types undefined - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `style` (M2)
* **Affected Scope:** lines 646, 649-652
* **Discrepancy Description:** The lease is `<home>/journal/<proc>.lock`, but `guarded(store, intent, fut)` and `begin_action(store, …)` take no home; `Store.path` is private and `None` for `in_memory` (`src/store.rs:649`). Where `proc` and the held `Lease` live, return types, and the fields of `Intent`, `Interrupted`, `EntryId` are undefined. "home" means three different folders across journal, protect and toolchain signatures.
* **Rationale & Recommendation:** Full signatures (for example `guarded<F: Future<Output = String>>(store, heron_home, intent, fut) -> String`, `static PROC: OnceLock<(String, Lease)>`, or `journal::init(home)`). Rename parameters `heron_home` / `user_home`.

### CD-M5 / CD-M6 / CD-M7 / SY9-M7 Run id and watch lifecycle cannot be built as written - Severity: Major
* **Reviewer Lens:** `codebase-details`, `style`; also Stage 5 ZO-4, Stage 6 CO-M4/CO-M13, Stage 8 Z8-3
* **Affected Scope:** lines 292, 753, 756, 772-773
* **Discrepancy Description:** `run_turn(&self, …)` (`src/agent.rs:468`) cannot assign a field; two `Agent {}` literals need updating (`src/app.rs:1213`, `src/agent.rs:847`). "Set at depth 0" overwrites `job:<id>`. `run_job` gets no run id; only `run_due_schedules` has one (`src/app.rs:1498-1500`); "Run now" (`app.rs:1703`) and triggers (`app.rs:1755`) create no `job_runs` row. Automations go through `call_tool` (`src/agent.rs:590-592`), never `run_turn`, so the depth-0 drop guard never calls `watch_end`.
* **Rationale & Recommendation:** Adopt Z8-3 (run frame owns the snapshot; reuse the events run id), or state the mechanism and precedence, define run ids for all three job paths, and call `watch_end` at the end of `run_automation`.

### CD-M8 The refusal reason has no route to the deny message - Severity: Major
* **Reviewer Lens:** `codebase-details`; also Stage 6 CO-M12
* **Affected Scope:** line 783; CUJ 5
* **Discrepancy Description:** `classify_path` returns a payload-free `Copy` enum `Effect` (`src/permissions.rs:19, 1117`); `Policy::decide` hard-codes the Refused text (`src/permissions.rs:384`).
* **Rationale & Recommendation:** Specify the channel (for example `decide` calls `protect::is_protected` again when `effect == Refused`).

### CD-M9 Failed shell commands count as successes in the loop guard - Severity: Major
* **Reviewer Lens:** `codebase-details`; also Stage 7 DC-M1
* **Affected Scope:** line 358; test `failing_call_with_changing_digits_stops`
* **Discrepancy Description:** Non-zero exit returns `Ok(text)` ending `[exit code N]` (`src/tools/shell.rs:124-134`, synthesizer verified), and a timeout returns `Ok` (line 107). The test cannot pass for `shell`.
* **Rationale & Recommendation:** Treat a trailing `[exit code N≠0]` and the timeout text as failures.

### CD-M10 `Resumer` with `async fn` cannot be `Arc<dyn Resumer>` - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness` (N1); also Stage 6 CO-N4
* **Affected Scope:** lines 332-335, 653
* **Rationale & Recommendation:** Add `#[async_trait]` (crate pattern, `src/permissions.rs:536`; `Cargo.toml:24`) to the fixed interface Harness2 binds to.

### CD-M12 Timeouts and cancellation unspecified - Severity: Major
* **Reviewer Lens:** `codebase-details`; also Stage 6 CO-M5, Stage 7 DC-M3
* **Affected Scope:** §1.3; file 7, file 8
* **Rationale & Recommendation:** Say whether `guarded` wraps the timeout and what a dropped future writes.

### CD-M13 W4 cannot reuse the sandbox's private path helpers - Severity: Major
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** file 4 line 706; Pre-S1
* **Discrepancy Description:** `subpaths`, `literals`, `both_forms`, `data_volume_alias` are private to `src/sandbox.rs:211-277`; W4's scope is `src/protect/**`. W4 would duplicate them or skip the `/System/Volumes/Data` forms, letting a write step around the rule.
* **Rationale & Recommendation:** Make them `pub(crate)` in Pre-S1 (same for egress socket subpaths).

### CD-M14 Workspace path interpolated into Seatbelt regex without escaping - Severity: Major
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** §3.1 line 383
* **Discrepancy Description:** `seatbelt_regex` (`src/sandbox.rs:141`) does not escape. A workspace path with `+`, `(` or `[` silently breaks or narrows the rule; a `"` breaks the profile.
* **Rationale & Recommendation:** Port anchor's `regex_escape` (`protect.rs:129-142`) and its quote test (`protect.rs:510-512`).

### IR-M5 / CD-M15 Heron's own API port deny has no implementation step - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `codebase-details`, `style` (M5); also Stage 6 CO-M17, Stage 8 Z8-6
* **Affected Scope:** OQ4 line 115; files 10-12; line 686
* **Discrepancy Description:** Nothing in `src/` writes kv `server:port`; no file section adds the write, the read, the parameter in `egress::seatbelt_rules`, or a test.
* **Rationale & Recommendation:** Either drop the deny (Z8-6, resting I3 on the token) or add the write in `server::start` (near `src/server.rs:255`), the read in `shell.rs`, the parameter, and `own_api_port_is_denied_in_the_profile`. Or use `cfg.server.port` (`src/config.rs:239`).

### CD-M16 "publish is already Forbidden" is wrong - Severity: Major
* **Reviewer Lens:** `codebase-details`
* **Affected Scope:** OQ5 line 121; A8 line 552
* **Discrepancy Description:** `src/permissions.rs:684` is in `danger_patterns()`, which returns `Privileged` (`src/permissions.rs:777-778`), not `forbidden_patterns()` (synthesizer verified). Publish asks; it is not Forbidden. OQ5's and A8's reasoning rest on it.
* **Rationale & Recommendation:** Correct both, and re-check OQ5 (in auto mode an approved publish still reaches an allowed host).

### CD-M17 / IR-M11 G8 conflict and no cross-platform check - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`; also Stage 5 ZO-8, Stage 6 CO-M20
* **Affected Scope:** G8 line 58; line 823; Verification Plan lines 850-860
* **Discrepancy Description:** inode/ctime are unix-only `MetadataExt`; anchor's `digest_into` uses `std::os::unix` (`protect.rs:193`). CI runs ubuntu, macos and windows (`.github/workflows/ci.yml:18`), `--features desktop` (`ci.yml:36`); the plan checks neither. No goal-to-test map; G1's `job:<id>` path and G5's inbox/audit rows have no test.
* **Rationale & Recommendation:** Decide the Windows variant or gate to unix; require the three-OS CI run green before the PR; add `cargo build --locked --features desktop --bins`; add a G1-G8 test map.

### CD-M18 / IR-M9 W6-W8 contract undefined; three Swift conformers - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`
* **Affected Scope:** files 11 and 13 (lines 806-809, 833-842)
* **Discrepancy Description:** No wire shape for `ToolchainState` or the four settings keys, no validation rules. `CoreClientProtocol` has three conformers (`CoreClient.swift:6`, `ManagedCoreClient.swift:12`, `PreviewCoreClient.swift:6`); no egress setter, though `test_egressHostListRoundTrips` needs one; `CoreSettings` (`Models.swift:560`) needs fields; test target unnamed (`Package.swift:19-24`).
* **Rationale & Recommendation:** Add JSON examples and validation to file 11; list protocol methods, wire models, all three conformers and the test target in file 13.

### CD-M19 / IR-M8 Settings types, validation and the `DEFAULT_ALLOW` dependency - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `style` (M1)
* **Affected Scope:** Pre-S1 line 627; line 689
* **Discrepancy Description:** Field types and validation in `apply_settings` (`src/server.rs:347`) unspecified; `max_repeats = 1` stops on the first call (`repeats.rs:132`). Pre-S1 needs `DEFAULT_ALLOW`, which W3 defines.
* **Rationale & Recommendation:** Put `DEFAULT_ALLOW` in Pre-S1's `egress/mod.rs`; give `max_repeats: u32 = 4` (min 2 or 0), `egress: String = "allowlist"`, `egress_allow: Vec<String>`, `deny_local_ports: Vec<u16>`.

### CD-M20 / IR-M6 Ported proxy interface and test list undefined - Severity: Major
* **Reviewer Lens:** `codebase-details`, `implementation-readiness`, `style` (M6)
* **Affected Scope:** lines 680, 690
* **Discrepancy Description:** anchor `start(allow, caps)` is `async` and returns `io::Result` (`proxy.rs:319`). 15 tests: two exercise caps (`:422, :439`), two need the unix bridge (`:754, :773`), one sets `Options{unix: true}` (`:711`). No anchor-harness commit pinned.
* **Rationale & Recommendation:** State the exact signature and that caps are dropped; list the 11 tests to keep; pin the commit.

### CD-M21 anchor's socket rules dropped without saying so - Severity: Major
* **Reviewer Lens:** `codebase-details`; also Stage 7 DC-M5
* **Affected Scope:** line 135; §3.2 lines 408-410
* **Discrepancy Description:** The port also drops anchor's syslog allow (`sandbox.rs:158-160`) and its `deny_sockets` list (Docker, Podman, OrbStack, Colima, Lima, `.gnupg`, 1Password; `sandbox.rs:132-144`). `/private/tmp` stays writable (synthesizer verified `src/sandbox.rs:386`), so sockets there are reachable.
* **Rationale & Recommendation:** Port `default_deny_sockets` and a per-command temp folder.

### IR-M7 Pre-S2 results go where nodes cannot read them - Severity: Major
* **Reviewer Lens:** `implementation-readiness`
* **Affected Scope:** line 631
* **Discrepancy Description:** PT-E2's extra hosts feed `DEFAULT_ALLOW` (W3) and `docs/cli-providers.md` (W7); PT-P1 decides a threat-model sentence; PT-T1 has no consumer. A PR description is in no node's worktree, and skein worktrees do not see commits made after a node starts.
* **Rationale & Recommendation:** Run Pre-S2 first and commit results into the doc before `skein run`; say what PT-T1 changes (for example `RUSTUP_AUTO_INSTALL=0`).

### IR-M10 / SY9-M3 / SY9-M12 Interfaces W6 needs from W4 and W5 are missing - Severity: Major
* **Reviewer Lens:** `implementation-readiness`, `style`, `codebase-details` (N12)
* **Affected Scope:** lines 704-708, 793-795, 819
* **Discrepancy Description:** No function returns the cached `Scan`; `watch_end` has no return type; `Scan`, `Snapshot`, `Changed` undefined (four names for one thing). `Profile` derives `Debug, Clone` (`src/sandbox.rs:69`), so new types need `Debug`, `Clone`, `Default`. "Empty defaults" and `launch` undefined. More `Profile::Shell` constructions exist in `shell.rs` tests (684, 768, 841, 1028, 1079).
* **Rationale & Recommendation:** Define `Scan`, `Snapshot { scan, hashes }`, `Snapshot::diff(&self, later) -> Vec<ProtectedChange>`, `cached_scan(session) -> Option<Scan>`, defaults `Scan::default()`, `None`, `ToolchainState::Trusted`.

### IR-M12 Scheduler-tick recovery must run only in the lock holder - Severity: Major (synthesizer: P2)
* **Reviewer Lens:** `implementation-readiness`; also Stage 6 CO-N7
* **Affected Scope:** line 771
* **Rationale & Recommendation:** "inside `if self.own_background_work()`, before `run_due_schedules`" (`src/app.rs:2134`).

### SY9-M9 / SY9-M10 Toolchain baseline rule and state names contradict tests - Severity: Major
* **Reviewer Lens:** `style`; also Stage 6 CO-M18
* **Affected Scope:** line 734 vs 462; lines 738, 800
* **Discrepancy Description:** `touch_without_content_change…` "refreshes the stat data", but only `trust_current` writes the baseline. "untrusted toolchain" is not a `ToolchainState` value; unclear if `NoBaseline` counts.
* **Rationale & Recommendation:** Allow refreshing stat data only for hash-matching files and amend line 462, or drop it; rename tests to `changed_toolchain_…`; state "`NoBaseline` produces no denies".

### SY9-M11 / SY9-M13 / SY9-M14 Tests placed in units that do not own the behaviour - Severity: Major (synthesizer: P2)
* **Reviewer Lens:** `style`, `implementation-readiness` (N5)
* **Affected Scope:** lines 664, 673, 778, 808, 846, 848, 879
* **Discrepancy Description:** W2's `nudge_threshold_is_three_identical_calls` tests a threshold that lives in `agent.rs` (W6). `unknown_resumer_fails_cleanly` (W1) asserts behaviour in `run_parked` (W6). `resume_action_calls_the_resumer_and_dismiss_calls_declined` (app.rs) covers a dismiss handled in `server.rs`. W7's grep requires a phrase its instructions do not require.
* **Rationale & Recommendation:** Rename or move each, as listed in the style table below; align W7's instruction with its grep.

### Codebase-details nits - Severity: Nit
* **Reviewer Lens:** `codebase-details`
* N1 "32 literal constructions" of `ToolCtx` is a raw grep count; about 22 struct expressions, 7 of them `..ctx`.
* N2 `content_hash` exists privately at `src/tools/fs.rs:63`; name one shared SHA-256 helper.
* N3 Nudge criterion narrows (fingerprint includes output; today `name:arguments`, `src/agent.rs:559`).
* N4 CUJ 3 "stopped:" vs anchor "stuck:" (`repeats.rs:75-76, 157`).
* N5 `state_survives_serialization` (`repeats.rs:136-145`) tests anchor's persisted format; meaningless for a per-turn guard.
* N6 anchor also allows `branch.*.remote|merge` and `remote.*.url|fetch` (`protect.rs:294-298`). Naming only `NEW_REPO_CONFIG_KEYS` makes every `git clone` report a change. (Synthesizer: P2.)
* N7 `is_protected` arity 3 (line 379) vs 4 (line 705).
* N8 `maintain()` holds `self.db()` (`src/store.rs:759`, synthesizer verified); calling `with_conn` inside deadlocks. (Synthesizer: P2.)
* N10 W6's filter omits `tools::fs::` and `mcp::`, whose tests cover `classify_path`; use `tools::`.
* N11 `mutates()` includes `Forbidden` (`src/permissions.rs:98`).

### Implementation-readiness nits - Severity: Nit
* **Reviewer Lens:** `implementation-readiness`
* N2 W7 greps one phrase per file; the threat model needs five items (lines 579-583).
* N3 Verify does not run `swift build` or the full Swift suite (line 852).
* N4 `egress::seatbelt_rules` does not cover the SSH-agent socket deny (line 410); no test.
* Scope overlap note: W6 `src/**` contains W1-W5, W7 `docs/**` contains verify's file. Serialized by depends-on, so safe; a later fix node for a W1-W5 directory cannot start while W6 holds `src/**`.

### Style nits (plan) - Severity: Nit
* **Reviewer Lens:** `style`
* N1 Verification Plan and verify row list different commands.
* N3 Test-name grep has no pattern and does not apply to Swift (line 866).
* N4 File 10 never names `seatbelt_profile`.
* N5 File 6 lists no tests though W6 filters `store::`; add `reopen_approval_sets_running_back_to_new`, `maintain_prunes_finished_rows_after_30_days`.
* N6 The second `toolchain::check` after the command is implicit (line 821); line 823 ambiguous on Linux/Windows steps.
* N7 `Store::db()` private stated twice (630, 748).
* N8 "PT-E1 to PT-T1" reads as a range (860).
* N9 Pre-S2 says what to do only on PT-E1 failure (631).
* N10 `Lease::is_dead` parenthetical (646).
* N11 Types and methods mixed in one list (672).
* N12 Two functions named `finish` (640, 650); rename `sql::write_outcome`.
* N13 "the notice" means two things (755).
* N14 Model-role cell format; W5 "profile denies" → `seatbelt_denies`; W6 "recovery" → name functions.
* N15 File 13 paths relative (833).
* N16 Test renames:

| Line | Current | Proposed |
|---|---|---|
| 660 | `a_live_process_intent_is_left_alone` | Drop, or rename 659 to `dead_process_intent_becomes_interrupted` |
| 665 | `unknown_resumer_fails_cleanly` | `unknown_resumer_sets_failed_with_unknown_handler_text` |
| 673 | `nudge_threshold_is_three_identical_calls` | `count_is_three_after_three_identical_calls` |
| 691 | `profile_denies_everything_but_loopback` | `rules_deny_outbound_except_loopback` |
| 695 | `default_list_has_no_crates_io_api_host` | `default_list_excludes_crates_io` |
| 715 | `heron_default_home_is_protected_when_home_is_overridden` | `default_heron_home_is_protected_when_heron_home_is_overridden` |
| 716 | `rules_cover_rename_of_a_parent` | `rules_return_repo_paths_for_refused_ancestors` |
| 724 | `Manifest::read(home)` | `Manifest::scan(user_home)` |
| 734 | `touch_without_content_change_is_not_a_mismatch` | `touch_without_content_change_stays_trusted` |
| 738, 800 | `untrusted_toolchain_…` | `changed_toolchain_…` |
| 739 | `trust_resets_state` | `trust_current_makes_state_trusted` |
| 758 | `ab_loop_stops_the_turn_with_a_notice` | `alternating_two_call_loop_stops_the_turn_with_a_notice` |
| 776 | `ask_again_rerun_rechecks_todays_rules` | `ask_again_rerun_rechecks_current_policy` |
| 777 | `ask_again_keeps_the_calls_taint` | `ask_again_keeps_the_original_call_taint` |
| 778 | `resume_action_calls_the_resumer_and_dismiss_calls_declined` | `run_again_on_resume_approval_calls_rerun` |
| 779 | `parked_approval_interrupted_is_asked_once` | `interrupted_parked_approval_is_asked_once` |
| 786 | `envrc_at_depth_is_refused` | `envrc_in_a_subfolder_is_refused` |
| 826 | `cargo_fetch_through_proxy_works` | `cargo_fetch_through_proxy_succeeds_with_default_allowlist` |

## Citations confirmed (codebase-details)

`src/agent.rs` 488, 558-566, 573-584, 590-597, 613, 685, 1361; `src/app.rs` 1788-1795, 2099-2117, 2113-2114; `src/store.rs` 190, 778, 1059-1300, 1312; `src/permissions.rs` 89, 1117, 1132; `src/sandbox.rs` 193-195, 255, 354/356, 387-388; `src/tools/shell.rs` 44-48, 64-69; `src/server.rs` 267-269, 1183-1186, 2269; `src/telegram.rs:973`; `src/config.rs:306`. Named symbols exist with stated signatures: `park_approval` (8 args in order), `claim_approval`, `recover_interrupted_jobs`, `recover_interrupted_approvals`, `Store::db` (private), `Store::in_memory`, `Store::maintain`, `kv_get`/`kv_set`, `STEPS`/`to_v3`, `own_background_work`, `scheduler_loop`, `run_parked`, `classify_path`, `seatbelt_profile`, `Profile::Shell { grants }`, `refused_ancestors`, `writable_extras`, `env_allowed`, `seatbelt_regex`. `answer_approval` is the only approval dismiss path. anchor `repeats.rs` is 204 lines with 6 tests; `proxy.rs` 792 lines; `AlreadyCommitted` at `effects.rs:68`; `protect.rs` has `WORKSPACE_NAMES`, `GIT_PARTS`, `NEW_REPO_CONFIG_KEYS`, `SKIP_DIRS`, `SCAN_LIMIT` = 100,000 (`HOME_PATHS` is new). `serde_json` is built without `preserve_order`, so `Value` keys serialize sorted.

**Harness2 interface:** Harness2's working copy does not name `register_resumer`, `begin_action`, `finish`, `Resumer`, `toolchain::check` or `ToolchainState`; its Pre-5 (line 1082) adopts "the names that PR gives them", so nothing conflicts by name. `triggers.state` has `interrupted`, `cancelled`, `setup_failed` (line 846). CD-M10 and the journal-home finding change the interface it would bind to.

## Not verifiable from the supplied context

testteeth CLI flags; rusqlite bundled SQLite `synchronous` default in WAL mode and the macOS `fsync` claim (busy timeout is set at `src/store.rs:608`); all Seatbelt behaviour (PT-E1, PT-P1, PT-T1); toolchain sizes and `sha2` throughput; whether PR #73 or `main` = `39dbdf5` matches the worktree; W8's zero-match exit code.
