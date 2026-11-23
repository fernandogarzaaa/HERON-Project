Status: resolved

# Goldfish Annotated Review Report

**Target:** `docs/designs/heron-harness-safety.md` (full file)
**Findings:** `consolidated_review_report.md` in this folder (67 findings: 3 P0, 17 P1, 34 P2, 13 P3), with the five stage reports beside it.
**Mode:** automated. No per-finding human gate: each decision below was made on technical merit. Calls only the owner can make went to the doc's Open Questions, not into the body.

**Rules applied**

*   OD1 to OD8 are kept byte for byte in the body. Where a finding argues an OD is wrong or the body reads it more narrowly or widely, the alternative or the reading is an Open Question (OQ1, OQ7 to OQ12), with a trade-off and a recommendation.
*   The invariants I1 to I4 hold. F02 (the resume path skipping the policy and taint checks) is fixed in §1.5.
*   Code was checked in this worktree (`src/` at `39dbdf5`) and in the open fix PRs #75 to #79 (`origin/fix/*`). Where the design depends on a PR, the doc says "after #NN".
*   Harness2 was read at `origin/docs/heron-harness2` = `e1ecd19`. Its file is the same at `origin/main` (`c9dd12c`).
*   Locations are section names, because line numbers moved with the edits.

## Counts

| Decision | Count |
|---|---|
| FIX | 66 |
| CONTEST | 0 |
| DISMISS | 1 |
| **Total** | **67** |

No finding was contested. The code and anchor-harness lines listed under "Evidence checked" were re-read for this resolution. Other P2 and P3 citations rest on the reviewers.

## Decisions

| ID | Sev | Decision | One line |
|---|---|---|---|
| F01 | P0 | FIX | The default follows OD5's wording: Heron's config, database and secrets files in `cfg.home` and the default `~/.heron` are protected for the file tools, not the folders, so Harness2's `selfheal/runs/` keeps working. Whole-folder protection is the alternative in OQ3, with the Harness2 trade-off. The false "both are already Refused" is corrected (§3.1). |
| F02 | P0 | FIX | `run_parked` handles `resume:` after `risk_notice::check`, `claim_approval` and the taint re-read. It sends the fixed effect `ProcessExec` through `policy.decide`, and a Deny sets `blocked`. `ActionIntent` gains `tainted`, which Harness2 fills from `origin_tainted`. `resume:` joins `sees_conversation_grants`. `rerun` receives the taint. §4 I2 and I4 are rewritten. Tests are in the W6a check (§1.5). |
| F03 | P0 | FIX | Every cargo check starts with `bash eval/require-tests.sh --lib <mod>:: -- <names>`. Run on today's tree, it exits 1; `cargo test -- --exact journal::tests::<name>` exits 0 ("0 passed; 486 filtered out"). The W8 check counts names in `swift test list` (Work units). |
| F04 | P1 | FIX | Pre-S1 writes the `effects` DDL as `step(conn: &Connection)`, re-pins `the_schema_cannot_change_without_a_new_version`, updates the version-3 assertion, adds `to_v4_adds_effects_and_keeps_rows`, lists the stub files, and checks with `cargo test --locked` (Pre-steps). |
| F05 | P1 | FIX | `Store::reopen_approval` moves to Pre-S1. `ask_again` calls `park_approval` or `reopen_approval` itself, outside `with_conn` (§1.4, file 1). |
| F06 | P1 | FIX | verify's scope is `src/**`. It runs last, so it can add the tests that kill surviving mutants (Work units). |
| F07 | P1 | FIX | Recovery runs in two passes. The first marks dead processes' rows `interrupted` in one transaction. The second is idempotent and runs at startup and on every tick: it asks again for each `interrupted` row with no `approval_id`, through `park_approval`'s de-duplication. Doc rule: `with_conn` closures never call `Store` methods (§1.4). |
| F08 | P1 | FIX | `heron.db*` and the lease files are left out of the rehash, because Heron writes them on every call. That is the smallest deviation from OD5; it is marked and is OQ8. The recommendation to drop the homes from the rehash entirely is in OQ8 too (§3.3). |
| F09 | P1 | FIX | `toolchain::export_baseline` and `import_baseline` let Harness2 seed the repair child, and the main process re-checks after the child exits. Harness2 calls `protect::Snapshot::take` and `diff` around the child. These dependencies are named in the header and in file 14 (§3.4, file 14). |
| F10 | P1 | FIX | A shell result also counts as a failure when it has a non-zero `[exit code N]` line or starts with `[timed out after`. This is a string rule, not a structured `execute_call` result (§2). |
| F11 | P1 | FIX | Built on #79, which denies outbound unix sockets except the system ones, the working folder and the shell's own `TMPDIR`. Egress then also denies mDNSResponder. anchor's daemon-socket denies are added for sockets inside the working folder. PT-E1 gains a socket listing, an `open`/`osascript`/`nscurl` mach-route test, and a check that #79's own-socket allows survive (§3.2, Prototype checks). |
| F12 | P1 | FIX | `guarded` owns the tool timeout (a timeout is `failed`) and has a drop guard: an abort writes `cancelled` with no ask-again, and a panic writes `failed` (after #76). The done/failed rule is defined. `watch_end` runs on a blocking thread (§1.3). |
| F13 | P1 | FIX | The `repair_transcript` text now says the call may have finished and an approval may be waiting. "Force Ask for an identical call while an ask-again is pending" is OQ9, default yes (§1.4, OQ9). |
| F14 | P1 | FIX | The manifest covers the whole toolchain folder except `lib/rustlib/src` and `share/doc`, so `libexec/` is in. The first check that sees a change posts the notice, whichever command made it. The after-check also runs on the timeout path. G7 states that a command already running is not covered (§3.4, G7). |
| F15 | P1 | FIX | anchor's `.git`-component refusal is ported for the file tools. Seatbelt denies `hooks/` entries under any `.git` at any depth, and re-allows only `*.sample` names under that same prefix. These rules come before the Refused and secret denies, so the allow re-opens nothing else. Config in a repository made during the run stays detector-only; the doc says so, and it is OQ7(e) (§3.1). |
| F16 | P1 | FIX | Each interpretation of an OD is now OQ7. The OD bullets are unchanged, so the "keep only the owner's words in OD3 and OD7" part was not applied; the draft's readings of OD3 and OD7 are OQ7 entries instead. The grouped draft parameters are OQ12 (Open Questions). |
| F17 | P1 | FIX | OQ1's default is OD7 as written. Its recommendation is the stronger option, which PT-T1 informs (only `toolchains/` is writable, so a sandboxed install may already fail). A11 and the header now agree, and the header says "defines", not "fixes" (OQ1, A11). Not done here: running PT-T1 now. It needs the owner's Mac unsandboxed, so Pre-S2 runs it before W5 and before the owner answers OQ1. |
| F18 | P1 | FIX | A task-local `RunFrame` set by the run's owner (depth-0 `run_turn` with no frame, `run_job`/`run_automation`, `run_parked`) carries the run id and the lazy snapshot. It survives `delegate`, `join_all` and `call_tool`; #77's spawn sets its own frame inside `run_parked`. The shell scans once per command (§1.2, §3.3). |
| F19 | P1 | FIX | The default keeps all of `.claude` (OD5 adopts anchor's list). Narrowing to `CLAUDE_HOME_ITEMS` is OQ10, recommended. Hitting the scan cap counts as a change and is named in the output (§3.1, OQ10). |
| F20 | P1 | FIX | G8 is reworded. The toolchain's inode and ctime fields, and anchor's execute-bit read in `protect.rs`, are `#[cfg(unix)]`. Windows runs no sandboxed commands, so its toolchain check returns `Trusted`. The checks add `cargo build --locked --features desktop --bins`, and PR CI must pass on all three OSes (after #78) (G8, Verification). |
| F21 | P2 | FIX | A "What protected paths break" list, with the message the model gets, sits next to §3.1. |
| F22 | P2 | FIX | anchor's names and the git hooks and config are treated as part of OD5. OQ3 keeps only the names that are actually new. |
| F23 | P2 | FIX | The deny covers `cfg.server.port` and every port this process's server bound. A fallback port bound by another process is a stated residual. I3 rests on the token (OQ4, §4). |
| F24 | P2 | FIX | `publish` is `Privileged` (it asks), not Forbidden. OQ5 and A8 are corrected and re-reasoned. |
| F25 | P2 | FIX | Defined: nodes use the `claude_build_rust` or `claude_build_swift` backend. skein runs each completion command itself, in the node's worktree (`heron-v1.md:823`). Pre-S3 starts `skein run` outside Claude's sandbox and records that it did. Node agents run the `--skip`-filtered tests; supervisor-only rows are marked (Run notes). |
| F26 | P2 | FIX | Pre-S2 runs first, and its results are committed into the doc's Prototype results before `skein run`. PT-T1's consumer is OQ1 (Pre-steps). |
| F27 | P2 | FIX | Field types, defaults and `apply_settings` validation are defined (`max_repeats` 0 or at least 2). `DEFAULT_ALLOW` moves to Pre-S1. |
| F28 | P2 | FIX | `guarded(store, heron_home, …)`. A process-wide lease per Heron home is taken before the first intent insert. A missing lease file means the process is dead. The types are defined (§1.4, file 1). |
| F29 | P2 | FIX | `#[async_trait]` is on `Resumer` (§1.5). |
| F30 | P2 | FIX | The `label` column, `session_id: String`, the `rerun` result mapping and the registry's home are defined. Stage 8's narrower `ask_again_action` changes G2, so it is OQ11 (§1.5). |
| F31 | P2 | FIX | Dismiss becomes a claim (`WHERE status='new'`), and `declined` is called only when the claim wins. Edited alongside #77's `answer_approval` (§1.5, file 11). |
| F32 | P2 | FIX | The full state machine: `intent` → `done`, `failed`, `cancelled` or `interrupted` → `asked` → `rerun` or `dismissed`. `args` is cleared at `asked`. `journal::answered` writes `rerun` or `dismissed` from `run_parked` and the dismiss claim. The prune keeps `intent`, `interrupted` and `asked` (§1.2, files 1, 8 and 11). |
| F33 | P2 | FIX | "Exactly one approval" now allows `park_approval`'s merge, and the crashed-job inbox note is stated (G1, §1.4). |
| F34 | P2 | FIX | anchor's `regex_escape` and its quote test are ported (§3.1, file 4). |
| F35 | P2 | FIX | Pre-S1 makes `subpaths`, `literals`, `both_forms` and `data_volume_alias` `pub(crate)`. |
| F36 | P2 | FIX | `Tool::protected` (default `None`, set by the file tools) fills `Call::refusal`, which `decide`'s Refused branch uses (file 9). |
| F37 | P2 | FIX | W4 ports anchor `protect.rs` whole at `daed7ef`. §3.1 and §3.3 list only the differences. One shared SHA-256 helper is used. |
| F38 | P2 | FIX | The signature is `pub async fn start(allow: Vec<String>) -> io::Result<EgressProxy>`. Caps and the unix bridge are dropped. The 11 tests to keep are named. anchor is pinned at `daed7ef` (file 3). |
| F39 | P2 | FIX | The user's list replaces the default. Invalid entries are refused with a message. The proxy fails closed. The false claim about background processes is corrected (§3.2). |
| F40 | P2 | FIX | Toolchain transitions are defined: stat data may be refreshed only for hash-equal files, `NoBaseline` adds no denies, and the tests are renamed `changed_toolchain_…` (§3.4). |
| F41 | P2 | FIX | "Part" and the skipped prefixes are defined. Anything else is left to the Seatbelt deny. The before-check stays (OD7) (§3.4). |
| F42 | P2 | FIX | One notice template. The nudge counts on name and arguments from the guard's own history, as today (§2). |
| F43 | P2 | FIX | `~/.cargo/git/checkouts` and the working folder's `.cargo/config.toml` are named in OQ2 and the Risks table. Working-folder build inputs stay out of scope. |
| F44 | P2 | FIX | Moved to OQ8. The default keeps OD5's rehash of home paths; the recommendation is workspace paths only. |
| F45 | P2 | FIX | The claim about a process that outlived its command is corrected. G5 now says "before the run's first mutating call". No snapshot is kept between runs. |
| F46 | P2 | FIX | Two phases. W6a wires U1, U2, the file-tool protection and the toolchain. W6b wires egress, the Seatbelt protect rules and the rehash, and waits on PT-E1 (Work units). |
| F47 | P2 | FIX | JSON shapes and validation are in file 11. File 13 lists the protocol methods, the wire models, all three conformers, the `CoreSettings` fields and the `HeronUITests` target. |
| F48 | P2 | FIX | `Scan`, `Snapshot`, `ProtectedChange` and one signature each are defined, deriving `Debug`, `Clone` and `Default`. Every `Profile::Shell` construction to update is listed (file 4, file 10). |
| F49 | P2 | FIX | Tick recovery runs inside `if self.own_background_work()`, the pattern #78 uses. "Within 20 seconds" (§1.4). |
| F50 | P2 | FIX | `maintain()` drops its guard before it prunes (file 6). |
| F51 | P2 | FIX | Tests are moved or renamed to the unit that owns the behaviour. W7 greps one marker per open item. |
| F52 | P2 | FIX | A Risks and mitigations table replaces the scattered lists. The prototype checks get their own `## Prototype checks` section. |
| F53 | P2 | FIX | One blocked-host string per refusal reason. G6 now says "a connection the proxy refuses is named". |
| F54 | P2 | FIX | A goal-to-test table is added, with tests for the `job:` path and G5's inbox and audit rows. verify runs `swift test`. |
| F55 | P3 | FIX | User-facing strings: CUJ 1 no longer doubles "Run", it is "Settings › Sandbox" everywhere, the refusal no longer uses "never", the notice covers all three report kinds, and no handler name is shown to the user. |
| F56 | P3 | FIX | "Run", "mutating call" and "user" are defined once in the Overview. The listed ambiguous phrases are rewritten. PT-E2 results are "must confirm". |
| F57 | P3 | FIX | §2 says a stopped sub-agent's closing text returns as the `delegate` result. |
| F58 | P3 | FIX | Home-path Seatbelt rules are emitted only for paths inside a writable folder. |
| F59 | P3 | DISMISS | Moving implementation detail out of §1 to §3 would touch every cross-reference the reviewers checked, for no change in behaviour. The detail stays, and new detail went into the plan. |
| F60 | P3 | FIX | The effects row records `workspace`. A re-run is refused when the current working folder differs. |
| F61 | P3 | FIX | Domain fronting and `cli_allow_agent_tools` shells are in the Risks table. |
| F62 | P3 | FIX | §1.4 records why `approvals.running` is not reused (no liveness test, `src/store.rs:1100-1105`). |
| F63 | P3 | FIX | Corrected: the `ToolCtx` count, `content_hash` exists, `mutates()` includes Forbidden, `delegate` is not journaled, `state_survives_serialization` is not ported. |
| F64 | P3 | FIX | Additional Resources and Future Improvements are H2. Harness2 links name the branch. t10, HQ2, PR #73, the Axiom proxy and `00-synthesis.md` are defined or dropped. The diagram's "or" is a word. Unlinked § references are linked. Not applied: heading case and uneven alternative shapes (cosmetic). |
| F65 | P3 | FIX | The Stage 9 renames table is applied. |
| F66 | P3 | FIX | The W6 filters use `tools::`. |
| F67 | P3 | FIX | The run notes now say W6a, W6b and verify hold `src/**`, so a fix node for W1 to W5 waits for them. No scope changed: depends-on already orders the rows. |

## Open questions written for the owner

*   **OQ1.** Default OD7 as written. Recommended: deny sandbox writes to toolchain `bin/`, `lib/` and `libexec/`.
*   **OQ3.** Default: protect the files that OD5 names in both homes. Alternative: protect the whole default `~/.heron`, which needs a Harness2 exemption.
*   **OQ7.** The draft's readings of OD1 to OD7: full arguments, `NetworkEgress` as a read, digits only in failures, `max_repeats = 0`, a lazy snapshot, new-repository config, `egress = "open"`, stat-first hashing, "uses", and the OD3 finding.
*   **OQ8.** What the rehash covers. Default: OD5's list minus `heron.db*`. Recommended: workspace paths only.
*   **OQ9.** Force Ask for an identical call while an ask-again is waiting. Default yes.
*   **OQ10.** Narrow `.claude` to `CLAUDE_HOME_ITEMS`. Default no (OD5); recommended yes.
*   **OQ11.** Stage 8's narrower Harness2 action interface. Default no (G2 as written).
*   **OQ12.** Draft parameters: 30-day retention, the default host list, port 443 only, leaving out `~/.cargo/bin`, the trust-on-first-use baseline, and no accepted-risk entry.

## Evidence checked

*   **F01:** `src/permissions.rs:949` (`refused = vec![heron_home…]`); `src/sandbox.rs:95`, `386-387`; Harness2 `e1ecd19` lines 420-421.
*   **F02:** `src/app.rs:1763-1816`: tool lookup, `tool.risk`, `policy.decide`, taint at 1788-1795.
*   **F03:** `eval/require-tests.sh` exits 1 on today's tree; `--exact` on a missing name exits 0.
*   **F04:** `src/store.rs:187` (`type Step = fn(&Connection)`), 190 (`STEPS`).
*   **F07:** `src/store.rs:1059-1064` (`park_approval` locks `self.db()` and merges).
*   **F10:** `src/tools/shell.rs:107` (timeout `Ok`), 124-134 (`[exit code N]`).
*   **F11:** `origin/fix/shell-sandbox-hardening` `local_sockets` and `shell_tmpdir`.
*   **F12:** `src/server.rs:645-653` (`handle.abort()`); `src/agent.rs:685-689`.
*   **F13:** `src/agent.rs:287`.
*   **F14:** `libexec/rust-analyzer-proc-macro-srv` is in both toolchains on the owner's Mac (`1.99.1` and `stable`).
*   **F15, F19, F37:** anchor `protect.rs:88-126` (the scan cut-off, `is_write_protected`), `185-200` (`digest_into`), `290-300` (the allowed `branch.*` and `remote.*` keys), and its test names.
*   **F20:** `.github/workflows/ci.yml:18` (three OSes), `36` (`--features desktop`); `src/sandbox.rs:438-444` (bubblewrap binds `writable_extras`).
*   **F18:** `src/agent.rs:468`, `590-597`, `717-736`; `src/app.rs:1496-1501` (`run_due_schedules` claims a `job_runs` row), `1538` (`run_job`), `1622` (`run_automation`).
*   **F23:** `src/server.rs:250-258` (fallback port), `124-137` (the token check on every route).
*   **F25:** `heron-v1.md:822-823`; `skein backends list` shows `claude_build_rust` and `claude_build_swift`; `skein run --help` shows `--verify-timeout` and that a backend timeout fails the node.
*   **F61:** `src/sandbox.rs:400-403` (the Cli profile writes only its run folder).
*   **F24:** `src/permissions.rs:684`, `777-778`.
*   **F31:** `src/server.rs:1183-1186`, `src/store.rs:1111-1114`.
*   **F36:** `src/permissions.rs:384`.
*   **F50:** `src/store.rs:758-759`.
*   **F57:** `src/agent.rs:729-734`.
*   **Swift:** `CoreClientProtocol.swift:6`, the three conformers, `Models.swift:560`, `Package.swift:19-21`.
*   **anchor-harness at `daed7ef`:** `protect.rs` constants; `sandbox.rs:132-160`, `217-237`; `proxy.rs:319` and its test names.
