# Consolidated Goldfish Review: heron-harness-safety.md

**Target:** `/Users/inan/Documents/PERSONAL/HERON-Project/.claude/worktrees/ultrafix-design/docs/designs/heron-harness-safety.md` (881 lines; Draft, 2026-11-22, read at `main` = `39dbdf5`). The document was not edited during the review.

**Flow:** the Uninterrupted Full Flow, with stages run one after another:
- 5 Zoom Out
- 6 Comprehension
- 7 Critic (design, structural, style)
- 8 Intermediate Zoom Out
- 9 Implementation Readiness (implementation-readiness, codebase-details, style)

Each reviewer was a fresh `egm:*` subagent. It got only the target path, the code paths, the persona and standard paths, and, for Stage 8 only, the earlier stage reports.

**Split:** FeatureRange is lines 1-619 and DetailedRange is lines 620-881. `## Detailed Implementation Plan` starts at line 620 and `## Work units` at line 869.

**Caveats**
* **Write location.** A worktree hook blocked writes into `ultrafix-design`. All six reports are therefore uncommitted in this session's worktree: `/Users/inan/Documents/PERSONAL/HERON-Project/.claude/worktrees/peaceful-goodall-2303aa/docs/designs/reviews/heron-harness-safety/`. Copy them to the requested folder.
* **Harness2 provenance.** No one could run `git show origin/docs/heron-harness2`, so reviewers read the working copy at `.claude/worktrees/serene-shamir-92b5d8/docs/designs/heron-harness2.md`. In `packed-refs`, that worktree's branch `feat/harness2-1a` is at `1fd6857`, while `origin/docs/heron-harness2` is at `e1ecd19`. The text may therefore differ from the revision this doc cites, and every Harness2 line number below refers to the working copy.
* **What the synthesizer checked directly.** For these findings, the synthesizer read the code or files and the finding held:

  | Finding | What was read |
  |---|---|
  | F01 | Harness2 working copy lines 420-421; `src/permissions.rs:949` |
  | F02 | `src/app.rs:1763-1816` |
  | F03 | the W7 grep strings have 0 matches in today's docs; that a 0-match `cargo test` filter exits 0 is standard cargo behaviour |
  | F04 | `src/store.rs:187`, `2040`, `2073` |
  | F07 | `src/store.rs:1059-1060` |
  | F10 | `src/tools/shell.rs:90-135` |
  | F11 | `src/sandbox.rs:386` |
  | F12 | `src/server.rs:645-653` |
  | F13 | `src/agent.rs:272-288` |
  | F14 | `libexec/rust-analyzer-proc-macro-srv` exists in both installed toolchains on the owner's Mac |
  | F15, F19 | anchor `protect.rs:90-125` |
  | F17 | `src/sandbox.rs:193-195` |
  | F18 | `src/agent.rs:468`, `590-592`; `src/app.rs:1499`, `1538`, `1703`, `1755` |
  | F20 | `.github/workflows/ci.yml:18`, `36`; `src/sandbox.rs:440-443` |
  | F24 | `src/permissions.rs:684`, `774-779` |
  | F50 | `src/store.rs:758-759` |

  F05, F06, F08, F09 and F16 are internal to the doc (plus the Harness2 lines above). Other P2 and P3 citations rest on the reviewers.

**Severity scale**
* **P0:** breaks an owner decision or invariant, breaks the Pre-5 contract that Harness2 depends on, or stops the build graph from proving that work was done. Fix before approval.
* **P1:** a Goal is not met as designed, or a unit cannot be built or checked as written. Fix before approval.
* **P2:** a gap or ambiguity that a builder would have to guess at. Fix before the build.
* **P3:** wording, structure, nit.
* Reviewer labels map roughly as Blocker = P0/P1, Major = P1/P2, Nit = P3. The synthesizer re-ranked each finding against this scale.

## Counts

| Severity | Count |
|---|---|
| P0 | 3 |
| P1 | 17 |
| P2 | 34 |
| P3 | 13 |
| **Total** | **67** |

## Top 5

1. **F01 (P0).** OQ3's default protects all of `~/.heron`. That Refuses every file write in Harness2's repair worktree at `~/.heron/selfheal/runs/<id>/wt`. Line 377's claim that both homes are "already Refused" is also false.
2. **F02 (P0).** `run_parked` routes `resume:<handler>` before the registry lookup. The Pre-5 action path therefore skips `policy.decide` and the taint re-read, so it enforces neither I2 nor I4.
3. **F03 (P0).** The checks for W1-W5 pass on the empty modules that Pre-S1 creates, because a `cargo test` filter that matches 0 tests exits 0. Nodes can be marked done with nothing built.
4. **F10 (P1).** A failing shell command returns `Ok("...[exit code N]")`. The stop for "failures that differ only in digits" (G3's second bullet, CUJ 4) therefore never fires for `shell`.
5. **F07 (P1).** Recovery's one-transaction promise contradicts the plan. `park_approval` takes the store mutex, and the scheduler tick never calls `ask_again`. Rows can end up `interrupted` with no approval, which breaks G1.

## Requested checks

### Work units table

| Row | Single-dir scope | Runnable check, exits 0 when done | Fails before the unit is built | Model role valid |
|---|---|---|---|---|
| W1-W5 | yes | yes (W3 runs only under the supervisor) | **no** (F03) | yes |
| W6 | yes, `src/**`, which contains W1-W5; safe because depends-on runs them in order (F67) | supervisor only (F25) | partly; it also fails for a reason the plan never mentions (F04) | yes |
| W7 | yes, `docs/**` | yes | yes (0 matches today) | yes |
| W8 | yes | supervisor only; exit code on 0 matching tests unverified | unverified | yes |
| verify | **no**: a single file (F06) | supervisor only | yes | yes |

Each model-role value (`hardest tasks: opus high`, `feature, refactoring: sonnet high`) matches an existing role line.

### Owner decisions OD1-OD8

| OD | Status | Where |
|---|---|---|
| OD1 | **Widened**: the full arguments are stored, while the owner listed only an args hash. "Never re-run silently" has two holes: a cancel or timeout leaves the intent open (F12), and the model can re-issue the call (F13). | F16, F12, F13 |
| OD2 | **Interpreted**: `NetworkEgress` is treated as a read, though the code describes it as sending data out. Defensible, but decided in the body rather than in an OQ. | F16 |
| OD3 | Kept in substance, but the draft's own "not cheap" finding sits inside the owner section. | F16 |
| OD4 | **Narrowed**: digits are ignored only in failures (line 359, "This draft decides it"). **Widened**: `max_repeats = 0` turns the stop off. As written, the digit rule cannot fire for shell. | F16, F10 |
| OD5 | **Narrowed**: the rehash runs only for runs that made a journaled call. **Widened**: hooks in repositories the agent creates are writable, against G4. **Contingent**: OQ3 presents anchor-harness's own list as "more than the owner listed". | F16, F15, F22 |
| OD6 | Kept on macOS. **Widened** by `egress = "open"`, and "accepted by OD6" claims breakage the owner never accepted. | F16 |
| OD7 | **Reinterpreted**: the hash check became stat-first, and "uses" was redefined (G7, line 478). OQ1 and A11 contradict each other. The manifest misses `libexec/`. | F16, F17, F14 |
| OD8 | Kept literally; G8 is contradicted. | F20 |

Some fixes that reviewers proposed would themselves weaken an OD. They are listed under "Dismissed or narrowed" below.

### Invariants I1-I4

* **I1 (sandbox on by default): kept.** No path turns the sandbox off; `egress = "open"` only drops the network rules.
* **I2 (tainted grants never reach the shell): kept for tool re-runs**, via `src/app.rs:1793-1795`. **Not enforceable on the `resume:` path** (F02). The local sockets in F11 are a way out of the sandbox, not a grant leak. A daemon that runs commands, reached that way, would still carry no taint.
* **I3 ("never" lifted only in the app): kept.** The argument partly relies on the API-port deny, which has no implementation and fails in the repair child (F23, F09). The token argument alone is enough.
* **I4 (memory never trusts outside content): kept for tool re-runs.** A resumer receives no taint, and could reach memory (F02).

## Prioritized findings

### P0

**F01. Protecting the default `~/.heron` locks Harness2's repair out of its own worktree (P0)**
* **Location:**
  * OQ3 line 111;
  * §3.1 lines 377 and 384;
  * the file 4 test at line 715.
* **Issue:**
  * Harness2's repair runs with `HERON_HOME=~/.heron/selfheal/runs/<id>/home`, and its worktree is at `~/.heron/selfheal/runs/<id>/wt` (working copy lines 420-421).
  * Under OQ3's default, `is_protected` Refuses every `write_file` and `edit_file` in that worktree.
  * If the "home paths" of line 384 reach Seatbelt, `cargo test` cannot write `target/` either.
  * Line 377 says "Both are already Refused". That is false: `ReadRoots::build` refuses only the configured home (`src/permissions.rs:949`).
* **Fix:**
  * Protect only the files `~/.heron/config.toml`, `heron.db*` and `secrets.json`, and only for the file tools.
  * Don't protect the folder, and leave the home out of the rehash. This also fixes F08.
  * Correct line 377.
  * Don't move Harness2's runs; that would breach OD8.
* **Sources:** ZO-1, DC-B2, CD-B1, Z8-2.

**F02. The `resume:<handler>` re-run skips the policy check and carries no taint (P0)**
* **Location:**
  * §1.5 lines 323 and 327-342;
  * §4, the I2 argument at line 492.
* **Issue:**
  * `run_parked` gets three things from the registry tool: its risk, the `policy.decide` call, and the taint re-read (`src/app.rs:1775-1803`).
  * Handling `resume:` "before the registry lookup" skips all three. Read-only mode, "never" rules and the current policy are then not applied.
  * `ActionIntent` has no `tainted` field, yet line 338 says the approval is "tainted from the intent".
  * `rerun(&App, subject)` receives no session and no taint.
* **Fix:**
  * Give every `resume:` entry one fixed declared effect: `ProcessExec`, which matches line 338's `"exec"`.
  * Build a `Call` and run `policy.decide` with the taint `run_parked` already computes. A Deny sets the status to `blocked`.
  * Add `tainted` to `ActionIntent`.
  * Stage 8 advises against a full context object, because Harness2's resumer only re-queues. A future resumer that does tool work must go through `Agent::call_tool`.
* **Sources:** DC-B1, Z8 Q1.

**F03. Work-unit checks pass before the unit is built (P0)**
* **Location:**
  * Work units lines 873-878 and 880;
  * Pre-S1 line 626.
* **Issue:**
  * Pre-S1 creates empty modules. `cargo test --locked --lib -- journal::` then matches 0 tests and exits 0. The same holds for `repeat::`, `egress::`, `protect::` and `toolchain::`.
  * W6's filter runs only tests that already exist, so it never proves the new tests exist.
  * W8's exit code when no test matches is unverified.
* **Fix:** each check first asserts that every named test is listed, then runs them:
  ```sh
  L=$(cargo test --locked --lib -- --list) && for t in <names>; do grep -q "$t: test" <<<"$L" || exit 1; done && cargo test --locked --lib -- journal::
  ```
  Do the same for W6 and W8.
* **Sources:** IR-B1, CD-N9.

### P1

**F04. Pre-S1 breaks two store tests, and the step signature doesn't fit `STEPS` (P1)**
* **Location:**
  * Pre-S1 lines 626-630;
  * file 1 line 639;
  * file 6 line 748.
* **Issue:**
  * Adding `to_v4` makes the schema version 4. Two tests then fail:
    * `the_schema_cannot_change_without_a_new_version` (`src/store.rs:2073`), which pins `(3, "878cc8a2270e5e69")`;
    * the version-3 assertion at `src/store.rs:2040`.
  * Pre-S1's check uses `--no-run`, so it never sees either failure.
  * No unit owns re-pinning the fingerprint, and there is no `to_v4` upgrade test.
  * `type Step = fn(&Connection) -> Result<()>` (`src/store.rs:187`) does not accept `step(tx: &Transaction)`.
  * "An empty `mod.rs`" conflicts with the `journal::sql::step` stub that Pre-S1 also needs.
* **Fix:**
  * Give one unit the job of re-pinning the fingerprint and adding `to_v4_adds_effects_and_keeps_rows`.
  * Use `pub fn step(conn: &Connection) -> Result<()>`.
  * List the stub files.
  * Pre-S1's check runs `cargo test --locked`.
* **Sources:** CO-M22, IR-B2, IR-M2, CD-M3, CD-M4, SY9-M1.

**F05. W1's test needs `reopen_approval`, which W6 writes (P1)**
* **Location:** lines 652, 663 and 746.
* **Issue:** `interrupted_parked_call_reopens_its_own_approval` is in W1, whose scope is `src/journal/**`. But `Store::reopen_approval` is added to `src/store.rs` in W6.
* **Fix:**
  * Move `reopen_approval` into Pre-S1, or into `journal::sql` through `with_conn`.
  * State whether `ask_again` calls `park_approval` or returns its arguments.
* **Sources:** CO-M24, IR-B3, CD-M1, SY9-M8.

**F06. Verify's scope is a file, not a directory glob (P1)**
* **Location:** lines 855 and 881.
* **Issue:** line 855 says surviving mutants are killed with new tests, but verify's scope (one docs file) cannot hold them. A testteeth score under 70 cannot be fixed inside the graph.
* **Fix:** give verify `src/**` (it runs last), or define a rule for `W-fix-<n>` nodes.
* **Sources:** IR-B4, SY9-N2.

**F07. Recovery's one-transaction promise can't be built as split; rows are left `interrupted` with no approval (P1)**
* **Location:**
  * §1.4 lines 308-313;
  * line 232;
  * file 1 lines 651-652;
  * file 8 lines 770-771.
* **Issue:**
  * `park_approval` takes the store mutex, which is not re-entrant (`src/store.rs:1059-1060`). It cannot be called inside `with_conn`.
  * File 8 splits the work into `recover` and then `ask_again`, but the scheduler tick calls only `recover`.
  * A crash between the two orphans the rows: the next `recover` lists only `intent` rows.
* **Fix:**
  * Add an idempotent second pass, at startup and on every tick: give an approval to every `interrupted` row whose `approval_id` is NULL.
  * Don't insert into `approvals` with raw SQL, which would copy `park_approval`'s de-duplication.
  * State that `with_conn` closures must not call `Store` methods.
* **Sources:** CO-B2, SY9-B1, IR-M3, CD-M2, Z8 Q1.

**F08. Rehashing Heron's own home reports a change after every run, or hashes gigabytes (P1)**
* **Location:**
  * §3.1 line 377;
  * §3.3 lines 436-445;
  * module table line 246;
  * Performance line 589.
* **Issue:** these files under Heron's home change routinely:
  * `heron.db` and its WAL;
  * the lease files and `background.lock`;
  * `config.toml`, on every Settings save;
  * Harness2 worktrees, 3-5 GB each.
* **Fix:** leave the home out of the rehash, as in F01's fix. If any home file stays in, list exactly which ones.
* **Sources:** ZO-3, CO-B1, Z8-2.

**F09. Repair runs keep toolchain and protect state in a throwaway home (P1)**
* **Location:**
  * §1.5 line 343;
  * §3.4 lines 461-463 and 481;
  * OQ4 line 115;
  * line 7;
  * file 14 line 848.
* **Issue:**
  * The child's trust-on-first-use baseline accepts whatever toolchain is installed when it starts.
  * Its notices and inbox entries are deleted along with its home.
  * kv `server:port` is read from the child's database, not the main one.
  * So G5 and G7 fail for the only unattended runs of model-written code, and line 481's claim does not hold.
  * Line 7's "two interfaces" also understates what Harness2 relies on.
* **Fix:**
  * Keep the toolchain baseline in one machine-wide place, or pass it to the child. This doc owns that.
  * The protect half belongs to Harness2 (OD8, as Stage 8 narrowed DC-M7): this doc exposes `protect::snapshot` and `changes`, and Harness2 calls them around the child.
  * Name these dependencies in line 7 and in file 14.
* **Sources:** ZO-2, CO-M19, DC-M7, Z8-9, Z8 Q1.

**F10. Failed shell commands count as successes, so the digits rule never fires for `shell` (P1)**
* **Location:**
  * §2 line 358;
  * CUJ 4 lines 175-180;
  * the test `failing_call_with_changing_digits_stops`.
* **Issue:** a non-zero exit returns `Ok(text)` ending in `[exit code N]` (`src/tools/shell.rs:124-134`). A timeout also returns `Ok` (line 107).
* **Fix:** add a string rule: a trailing `[exit code N≠0]`, or the timeout text, counts as a failure. Stage 8 advises against changing `execute_call` to return a structured result.
* **Sources:** DC-M1, CD-M9, Z8-7.

**F11. Sockets in the writable temp folders are a way around egress, protected paths and the toolchain deny (P1)**
* **Location:**
  * §3.2 lines 135 and 408-412;
  * A6 line 539.
* **Issue:**
  * The rules allow socket connects "inside the writable folders". Those include all of `/private/tmp` and `/private/var/folders` (`src/sandbox.rs:386`).
  * anchor-harness uses a per-run temp folder instead. It also denies the Docker, Podman, OrbStack, Colima, Lima, `.gnupg`, 1Password and `SSH_AUTH_SOCK` sockets (`sandbox.rs:132-151, 227-231`). The port drops that list without saying so.
  * `(allow default)` leaves mach-lookup open, and that route is untested.
* **Fix:**
  * Port `default_deny_sockets` and the per-command temp folder, and set `TMPDIR` to it.
  * Add a PT-E1 step that lists the sockets on the owner's Mac and tests a fetch through a mach service.
  * Raise this to P0 if that step finds a socket that runs commands.
* **Sources:** DC-M5, CD-M21.

**F12. A cancel or timeout leaves intents open under a live lease (P1)**
* **Location:**
  * §1.3-1.4 lines 296-311;
  * line 442.
* **Issue:**
  * `POST /api/cancel` aborts the run's task (`src/server.rs:645-653`). The tool timeouts (`src/agent.rs:685`, `src/app.rs:1804`) drop the future. Neither writes an outcome.
  * The row keeps its full arguments for weeks, then surfaces as a stale "Run again?".
  * How `done` and `failed` are told apart is undefined.
* **Fix:**
  * A drop guard in `guarded` writes `cancelled`. A cancel the user started gets no ask-again.
  * Put the timeout inside `guarded`.
  * Define the done/failed rule.
  * Run `watch_end` on a blocking thread.
* **Sources:** CO-M5, DC-M3, CD-M12.

**F13. The model can re-issue an interrupted call itself (P1, OD1)**
* **Location:** G1 line 49; OD1 line 75.
* **Issue:**
  * `repair_transcript` fills the gap with "[interrupted before this tool finished; no result]" (`src/agent.rs:287`).
  * After a restart, a user saying "continue" in auto mode can get `git commit` re-run with no ask.
  * If the user also presses Run again, it runs twice.
* **Fix:**
  * For journaled calls, change the inserted text to say the call may have finished and an approval is waiting.
  * Put "force Ask for an identical call while an ask-again is pending" to the owner as an OQ.
* **Sources:** DC-M4, Z8 Q1.

**F14. The toolchain manifest misses `libexec/`, and the notice can be skipped (P1, OD7 and G7)**
* **Location:**
  * §3.4 lines 457 and 476-479;
  * G7.
* **Issue:**
  * rust-analyzer runs `libexec/rust-analyzer-proc-macro-srv` unsandboxed. It is present in both installed toolchains, and the manifest does not cover it.
  * No notice is raised when the change comes from any of these:
    * a command that was killed or timed out (the early return at `src/tools/shell.rs:94-108`);
    * a background process;
    * a command running at the same time.
* **Fix:**
  * Cover the whole toolchain folder except `lib/rustlib/src` and `share/doc`.
  * Notify on any first detection.
  * Run the after-command check on the timeout path too.
  * State in G7 that commands running at the same time are not covered.
* **Sources:** DC-M6.

**F15. The file-tool git rule is weaker than anchor-harness's, and agent-created repositories are a hole (P1, OD5 and G4)**
* **Location:** §3.1 lines 369-372 and 379.
* **Issue:**
  * anchor refuses any file-tool path that has a `.git` component (`protect.rs:117-125`). This design protects only the hooks and config of repositories that already exist.
  * That leaves these routes open:
    * write `x/hooks/pre-commit`, then `sub/.git` containing `gitdir: ../x`;
    * `.git/modules/<n>/hooks`;
    * line 372's allowance for repositories the agent creates, which widens OD5 and contradicts G4.
* **Fix:**
  * Port the `.git`-component refusal for the file tools.
  * In Seatbelt, deny writes under `.git/hooks` at any depth except names ending `.sample`.
  * Or put the new-repository exception to the owner as an OQ.
* **Sources:** DC-M10, Z8-1.

**F16. Draft decisions are made outside Open Questions, against line 73 (P1)**
* **Location:**
  * **OD1:** line 284.
  * **OD2:** line 263.
  * **OD3:** line 77.
  * **OD4:** lines 359-360 and G3 (line 53).
  * **OD5:** G5 (line 55) and line 436.
  * **OD6:** lines 414-419 and 425.
  * **OD7:** lines 81 and 465-466, G7 (line 57), and line 478.
  * **OQ3's default built into the plan:** line 377 and the test at line 715.
* **Fix:**
  * Add one OQ entry, with a proposed default, for each change to an OD.
  * Keep only the owner's words in OD3 and OD7.
  * Add one grouped "Draft parameters" OQ for the rest: 30-day retention, the default host list, port 443 only, leaving out `~/.cargo/bin`, the trust-on-first-use baseline, and no accepted-risk entry.
* **Sources:** ZO-6, ST-B1, ST-M2, ST-M3, ST-M4, SY-B13, ST-N12.

**F17. OQ1 is unsettled while §3.4 is presented as a fixed Pre-5 interface (P1)**
* **Location:**
  * line 7;
  * OQ1 lines 95-102;
  * PT-T1 line 144;
  * A11 line 568.
* **Issue:**
  * OQ1's stronger option has no default ("The owner decides"), but A11 calls it "the recommended change".
  * Adopting it deletes most of §3.4, W5, and Harness2's call to `toolchain::check`.
  * Line 44's premise is unproven. Only `.rustup/toolchains` is writable, not the rest of `.rustup` (`src/sandbox.rs:194`), so a sandboxed install may already fail.
* **Fix:**
  * Run PT-T1 now.
  * Give the stronger option a proposed default, and put the cost comparison to the owner before calling §3.4 fixed.
* **Sources:** ST-M5, Z8-5.

**F18. Run state is placed on the wrong objects (P1)**
* **Location:**
  * §1.2 line 292;
  * §3.1 line 387;
  * §3.3 lines 436-442;
  * files 7 and 8, lines 753, 756 and 772-773.
* **Issue:**
  * `run_turn(&self)` (`src/agent.rs:468`) cannot set a field on the agent.
  * Setting the run id at depth 0 overwrites `job:<id>`.
  * `run_job` gets no `job_runs` id from "Run now" (`src/app.rs:1703`) or from triggers (`:1755`).
  * Automations go through `call_tool` (`src/agent.rs:590-592`) and never reach `run_turn`, so nothing calls `watch_end` for them.
  * A watch keyed by session id collides between a turn and `run_parked`.
  * A lazy `watch_begin` contradicts G5's "at the start of the run".
* **Fix:**
  * Let the run's own frame own the snapshot, taken at run start: the depth-0 `run_turn`, `run_parked`, or the job runner.
  * Reuse `crate::events::new_run_id()`.
  * Scan once per shell command.
  * Call `watch_end` at the end of automations.
* **Sources:** ZO-4, CO-M4, CO-M13, SY-B1, SY-B2, CD-M5, CD-M6, CD-M7, SY9-M7, Z8-3.

**F19. Protecting `.claude` at any depth, hashed file by file, collides with this repository's layout, and the scan cap fails open (P1)**
* **Location:**
  * §3.1 lines 368 and 387;
  * §3.3 lines 436-439;
  * Performance line 589.
* **Issue:**
  * When the Heron repository root is the working folder, the shell can no longer write `.claude/worktrees/**`.
  * The run's first mutating call hashes whole worktrees.
  * The scan returns early at 100,000 entries (`protect.rs:94-97`). Repositories found after that point get no deny.
* **Fix:**
  * Put a narrowing of `.claude` to the owner as an OQ, for example anchor's `CLAUDE_HOME_ITEMS`, since OD5 covers the whole folder.
  * Treat hitting the cap as a change.
  * Stage 8 rejects byte caps that fail open.
* **Sources:** DC-M9, Z8-11, CO-M15.

**F20. G8 is contradicted, and nothing checks Linux or Windows (P1)**
* **Location:**
  * G8 line 58;
  * line 67;
  * line 823;
  * Verification Plan lines 850-860.
* **Issue:**
  * The toolchain refusal runs on Linux and Windows too, but only the Mac app can re-trust a toolchain.
  * inode and ctime come from unix-only `MetadataExt`. A direct port breaks the Windows build.
  * CI runs on ubuntu, macos and windows, with `--features desktop` (`.github/workflows/ci.yml:18, 36`). The plan checks neither.
* **Fix:**
  * Reword G8. Don't limit the check to macOS: bubblewrap binds `writable_extras`, including `.rustup/toolchains`, as writable (`src/sandbox.rs:440-443`).
  * Make the stat fields platform-specific.
  * Require a green CI run on all three operating systems.
  * Add `cargo build --locked --features desktop --bins` to the checks.
* **Sources:** ZO-8, CO-M20, CD-M17, IR-M11, Z8 Q4.

### P2

**F21. No "what breaks" list for protected paths (P2)**
* **Location:** §3.1 lines 364-387; OQ3 lines 107-111.
* **Issue:** the rules as written break several everyday actions:
  * the `.git/config` deny breaks `git remote add`, `git push -u`, `git branch -u`, `git config` and submodule init;
  * husky's `prepare` script fails, so `npm install` fails;
  * edits to `.vscode`, `.claude` and `.envrc` are refused.
* **Fix:** add the list next to OQ3, with the message the model gets for each item.
* **Sources:** ZO-5, CO-M15, DC-M12.

**F22. OD5 depends on how OQ3 is framed (P2)**
* **Location:** OD5 line 79; OQ3 lines 107-111.
* **Issue:**
  * OQ3 presents `.gitmodules`, `.husky`, `.claude`, `.vscode`, `.idea`, `.ripgreprc` and git config as more than the owner asked for. They are already in anchor's list (`protect.rs:25-43`), which OD5 adopts.
  * LaunchAgents is not in anchor's list.
* **Fix:** treat anchor's names as part of OD5 itself, and keep OQ3 for names that are actually new.
* **Sources:** ZO-7.

**F23. The API-port deny has no writer (P2)**
* **Location:**
  * OQ4 line 115;
  * §3.2 line 407;
  * I3 line 497;
  * files 10-12.
* **Issue:**
  * Nothing writes kv `server:port`.
  * The deny fails when two servers run, and in the repair child.
* **Fix:** either drop the deny and rest I3 on the token, or add the write, the read and the test `own_api_port_is_denied_in_the_profile`.
* **Sources:** CO-M17, IR-M5, CD-M15, DC-N6, SY9-M5, Z8-6.

**F24. "publish is already Forbidden" is wrong (P2)**
* **Location:** OQ5 line 121; A8 line 552.
* **Issue:** `danger_patterns()` maps `publish` to `Privileged`, which asks the user; it is not `Forbidden` (`src/permissions.rs:684, 777-778`).
* **Fix:** correct both lines and re-check OQ5's reasoning.
* **Sources:** CD-M16.

**F25. "The supervisor runs them" is not defined here (P2)**
* **Location:** lines 697, 824 and 864-867.
* **Issue:** the W3 and W6 agents get no test feedback while they build.
* **Fix:**
  * Cite the supervisor mechanism and the backend, and say what a failing check does.
  * Split each check into tests the node can run and supervisor-only tests (skip the network tests in the node with `--skip <submodule>`).
  * Mark the supervisor-only rows in the table.
* **Sources:** IR-M1, CO-M23, CO-N9.

**F26. Pre-S2 results never reach the build nodes (P2)**
* **Location:** line 631.
* **Issue:**
  * The plan puts the prototype results in the PR description, which is in no node's worktree.
  * Nothing in the plan uses PT-T1's result.
* **Fix:** run Pre-S2 first, and commit its results into the doc before `skein run`.
* **Sources:** IR-M7, SY9-N9.

**F27. Config field types and validation are missing (P2)**
* **Location:** Pre-S1 line 627; line 689.
* **Issue:**
  * `DEFAULT_ALLOW` is needed in Pre-S1, but W3 defines it.
  * `max_repeats = 1` stops the turn at the first call.
* **Fix:** give the field types, defaults and `apply_settings` validation, with `max_repeats` either 0 or at least 2.
* **Sources:** IR-M8, CD-M19, DC-N2.

**F28. The journal API has nowhere to hold the lease (P2)**
* **Location:** lines 646-652.
* **Issue:**
  * `guarded` and `begin_action` take no Heron home, so they don't know where the lease file goes.
  * `Intent`, `Interrupted` and `EntryId` are undefined.
  * The lease ordering and the missing-file case are unspecified.
* **Fix:**
  * Add `journal::init(heron_home)` or a home parameter.
  * Take the lease before the first intent insert.
  * Treat a missing lease file as a dead process.
* **Sources:** IR-M4, CD-M11, SY9-M2, CO-M7, DC-N3.

**F29. The `Resumer` trait needs `#[async_trait]` (P2)**
* **Location:** lines 332-335.
* **Issue:** a trait with an `async fn` can't be used as `Arc<dyn Resumer>` without `#[async_trait]`.
* **Fix:** add `#[async_trait]` to the interface Harness2 binds to.
* **Sources:** CO-N4, CD-M10, IR-N1.

**F30. The action-entry interface is underspecified (P2)**
* **Location:** §1.5 lines 325-347.
* **Issue:**
  * `label` has no column in the table.
  * The session type is undefined.
  * How `rerun`'s result maps to an approval status is undefined.
  * Where the registry lives is undefined.
* **Fix:** define these, or adopt Stage 8's narrower `ask_again_action`, with Harness2 keeping its own `interrupted` state. That changes G2, so it needs an OQ.
* **Sources:** CO-M10, ZO-11, Z8-4.

**F31. Dismiss can race `declined` (P2)**
* **Location:** §1.5 line 340.
* **Issue:**
  * Dismiss is unconditional (`src/server.rs:1183-1186`, `src/store.rs:1111-1114`).
  * A stale Dismiss can cancel a repair that was just re-queued.
* **Fix:** make Dismiss a claim (`WHERE status='new'`), and call `declined` only when the claim wins.
* **Sources:** DC-M2.

**F32. The effects state machine stops at `asked` (P2)**
* **Location:**
  * §1.2 lines 280-285;
  * Security line 576;
  * test line 667.
* **Fix:**
  * Define the states after `asked`.
  * Say which states the prune keeps as open.
  * Say when `args` is set to NULL for an interrupted row.
* **Sources:** CO-M6, SY-C6.

**F33. "Exactly one approval" conflicts with `park_approval`'s de-duplication (P2)**
* **Location:** G1 line 49; §1.4 line 314.
* **Issue:**
  * `park_approval` merges identical rows (`src/store.rs:1061-1064`).
  * A crashed job also gets an inbox note on top of its approvals.
* **Fix:** state the intended behaviour.
* **Sources:** CO-M8, ZO-14.

**F34. The workspace path is not escaped in the Seatbelt regex (P2)**
* **Location:** §3.1 line 383.
* **Issue:** a `+`, `(` or `[` in the path silently narrows the rule.
* **Fix:** port anchor's `regex_escape` and its quote test.
* **Sources:** CD-M14.

**F35. W4 cannot reach the sandbox path helpers (P2)**
* **Location:**
  * file 4 line 706;
  * Pre-S1 lines 624-630;
  * W4 line 876.
* **Issue:** `subpaths`, `literals`, `both_forms` and `data_volume_alias` are private (`src/sandbox.rs:211-277`). W4 risks missing the `/System/Volumes/Data` forms of a path.
* **Fix:** make them `pub(crate)` in Pre-S1.
* **Sources:** CD-M13.

**F36. The refusal reason has no route to the deny message (P2)**
* **Location:** file 9 line 783; CUJ 5 line 187.
* **Issue:** `Effect` carries no payload, and `decide` hard-codes the Refused text (`src/permissions.rs:384`).
* **Fix:** name how the reason reaches the deny message.
* **Sources:** CO-M12, CD-M8.

**F37. W4 rewrites anchor-harness's `protect.rs` instead of porting it (P2)**
* **Location:**
  * §3.1 lines 364-387;
  * §3.3 lines 434-452;
  * file 4 lines 699-718;
  * W4 line 876;
  * line 354.
* **Issue:**
  * The rewrite reopens questions the tested port already settles: diff precedence and deletion, and how file tools find repositories.
  * Without anchor's allowed `branch.*` and `remote.*` config keys, every `git clone` reports a change.
* **Fix:**
  * Port the whole file, as W2 and W3 already do.
  * Use one shared hash helper (one exists at `src/tools/fs.rs:63`).
  * Cut §3.1 and §3.3 down to the differences from anchor.
* **Sources:** Z8-1, CO-M11, CO-M14, CD-N6, ZO-10.

**F38. The ported proxy interface is undefined (P2)**
* **Location:** file 3 lines 679-690; §3.2 line 395.
* **Issue:** anchor's `start` is `async fn start(allow, caps) -> io::Result` (`proxy.rs:319`). The plan writes `start(allow) -> EgressProxy`.
* **Fix:**
  * State the exact signature.
  * Say that caps are dropped.
  * List the 11 tests to keep.
  * Pin the anchor commit.
* **Sources:** CO-N5, IR-M6, CD-M20, SY9-M6.

**F39. The egress settings' behaviour is undefined (P2)**
* **Location:** §3.2 lines 401-403 and 414-419.
* **Issue:**
  * The doc doesn't say whether a user's list replaces the default or extends it, or what an invalid value does.
  * It doesn't say whether the proxy fails closed when it can't start.
  * Line 403 is false: under `localhost:*`, a background process can reach a later command's proxy.
* **Fix:** define each case, with failing closed as the default.
* **Sources:** CO-M16, DC-N5.

**F40. The toolchain state transitions are undefined (P2)**
* **Location:**
  * §3.4 lines 462 and 470-479;
  * file 5 tests at lines 734 and 738;
  * file 10 line 800.
* **Issue:**
  * After a first-use baseline, the after-command notice can never fire.
  * A test refreshes stat data, although the doc says only `trust_current` writes the baseline.
  * The tests say "untrusted", which is not a `ToolchainState` value.
* **Fix:** define the transitions, and rename the tests to `changed_toolchain_…`.
* **Sources:** CO-M18, SY9-M9, SY9-M10.

**F41. Rust-tool detection is undefined (P2)**
* **Location:** §3.4 line 477.
* **Issue:** "the first word of any part of the command" defines neither the separators nor how `env`, `VAR=x` and `xargs` prefixes are handled.
* **Fix:**
  * Define "part" and those prefixes.
  * Or drop the command-line parser and turn the Seatbelt denial into the friendly message.
  * Either way, keep the check before each command, as OD7 requires.
* **Sources:** CO-M21, ZO-12, SY-C3.

**F42. The loop-guard notice texts disagree, and the nudge's trigger changes without saying so (P2)**
* **Location:**
  * CUJ 3 line 172;
  * CUJ 4 line 179;
  * §2 lines 356-357;
  * A4 line 527.
* **Issue:**
  * CUJ 3 says "stopped:", while CUJ 4 and anchor say "stuck:".
  * Line 357 nests `<reason>` inside parentheses.
  * `count()` includes the call's output, so the nudge fires in fewer cases than today, when it keys on `name:arguments` only.
* **Fix:** use one notice template, and count the nudge on name plus arguments.
* **Sources:** SY-A1, CO-M1, CO-M2, ZO-13, CD-N3, CD-N4, DC-N1.

**F43. Routes in the same class as OQ2 are not named (P2)**
* **Location:**
  * OQ2 lines 103-106;
  * §3.4 lines 457-460;
  * Security lines 578-583.
* **Issue:**
  * `~/.cargo/git/checkouts` carries the same risk as `~/.cargo/registry/src`.
  * The working folder's `.cargo/config.toml` can set `rustc-wrapper`.
* **Fix:** name both in OQ2 and in the threat model. Stage 8 rejects protecting build inputs in the working folder.
* **Sources:** DC-M11.

**F44. Rehashing home paths raises false alarms (P2)**
* **Location:**
  * §3.1 lines 373-376;
  * §3.3;
  * G5 line 55;
  * CUJ 6.
* **Issue:**
  * Installers, `gh` and IDEs write these files routinely.
  * The sandbox's write rules already deny sandboxed commands write access to them.
* **Fix:** propose in an OQ that the rehash covers workspace paths only.
* **Sources:** Z8-10.

**F45. Line 452's "process that outlived its command" claim is false (P2)**
* **Location:**
  * §3.3 lines 436 and 452;
  * G5 line 55.
* **Issue:** each run compares against a fresh snapshot of its own, so a change made between runs is never seen.
* **Fix:** correct the claim and align G5's wording. Stage 8 rejects keeping a snapshot between runs.
* **Sources:** DC-M8, ST-M8.

**F46. A PT-E1 failure would stop the journal too (P2)**
* **Location:** Pre-S2 line 631; W6 line 878.
* **Issue:** if PT-E1 fails, the plan stops before any code. That holds back the effect journal, which doesn't depend on egress.
* **Fix:** build in two phases and split W6 to match:
  * **Phase 1:** U1, U2, file-tool protection, and the toolchain decision.
  * **Phase 2:** egress, the Seatbelt protect rules, and the rehash.
* **Sources:** Z8-8.

**F47. The W6-W8 contract is undefined (P2)**
* **Location:** file 11 lines 803-812; file 13 lines 833-842.
* **Issue:**
  * There are no JSON shapes and no validation rules.
  * There are three `CoreClientProtocol` conformers, not one.
  * There is no setter for egress.
  * `CoreSettings` needs new fields.
  * The test target is not named.
* **Fix:**
  * In file 11, give JSON examples for `GET /api/toolchain` and the four settings keys, with the validation and error text.
  * In file 13, list:
    * the protocol methods: `toolchain()`, `trustToolchain()` and `setEgress(mode:hosts:denyPorts:)`;
    * the wire models;
    * all three conformers;
    * the `CoreSettings` fields;
    * the test target.
* **Sources:** CD-M18, IR-M9.

**F48. W6 needs interfaces that W4 and W5 don't define (P2)**
* **Location:**
  * file 4 lines 704-708;
  * file 10 lines 793-795;
  * file 12 line 819;
  * lines 379 against 705, and 381 against 706.
* **Fix:**
  * Define these:
    * `Scan`;
    * `Snapshot { scan, hashes }`;
    * `Snapshot::diff(&self, later) -> Vec<ProtectedChange>`;
    * `cached_scan(run) -> Option<Scan>`;
    * `watch_end -> Option<Vec<PathBuf>>`.
  * Derive `Debug`, `Clone` and `Default` on them (`src/sandbox.rs:69`).
  * Use one signature each for `is_protected` and `seatbelt_rules`.
  * List every `Profile::Shell` construction to update, including the `shell.rs` tests at lines 684, 768, 841, 1028 and 1079.
* **Sources:** IR-M10, SY9-M3, SY9-M4, SY9-M12, CD-N7, CD-N12, CO-N3.

**F49. Tick recovery must run only in the process that holds the lock (P2)**
* **Location:** file 8 line 771; §1.4 line 308.
* **Issue:** the plan says "on each tick" without naming the process. The tick runs every 20 s, not "within a minute" as the doc says.
* **Fix:** call it inside `if self.own_background_work()` (`src/app.rs:2134`).
* **Sources:** IR-M12, CO-N7.

**F50. `maintain()` calling `with_conn` would deadlock (P2)**
* **Location:** file 6 line 747.
* **Issue:** `maintain()` holds `self.db()` (`src/store.rs:759`), so calling `with_conn` inside it deadlocks.
* **Fix:** release the guard before pruning.
* **Sources:** CD-N8.

**F51. Tests sit in units that don't own the behaviour, and W7's grep doesn't match its instructions (P2)**
* **Location:** lines 664, 673, 778, 846 and 879.
* **Fix:**
  * Rename or move each test; the Stage 9 report has the renames.
  * Give W7 one grep marker per open item.
* **Sources:** SY9-M11, SY9-M13, SY9-M14, IR-N2, IR-N4, IR-N5.

**F52. Risks are scattered, and the prototype checks sit inside Open Questions (P2)**
* **Location:**
  * lines 126-144;
  * OQ1, OQ2, OQ4 and OQ5;
  * §3.2 lines 425-432;
  * Security lines 578-583.
* **Fix:**
  * Add one Risks and Mitigations table that replaces the scattered lists.
  * Move the prototype checks into a `## Prototype checks` section.
* **Sources:** ST-M6, ST-M7.

**F53. The blocked-host line gives one reason for four refusals, and G6 overpromises (P2)**
* **Location:**
  * G6 line 56;
  * CUJ 7 line 205;
  * §3.2 lines 395-398 and 428.
* **Issue:**
  * The proxy refuses for four reasons, but there is only one blocked-host message.
  * Programs that bypass the proxy produce no message at all.
* **Fix:**
  * Give each reason its own string.
  * Reword G6 to: "a connection the proxy refuses is named in the command's output."
* **Sources:** SY-A3, CO-N6.

**F54. The Verification Plan has no goal-to-test map (P2)**
* **Location:** Verification Plan lines 850-860; verify row line 881.
* **Issue:**
  * No test covers G1's `job:` path, or G5's inbox and audit rows.
  * verify does not run the Swift build or suite.
* **Fix:**
  * Add a G1-G8 to test table.
  * Add tests for the `job:` run path and for G5's inbox and audit rows.
  * Add `swift test --package-path macos/HeronMac` to verify, run by the supervisor.
* **Sources:** IR-M11, IR-N3, SY9-N1.

### P3

| ID | Location | Issue | Fix | Sources |
|---|---|---|---|---|
| F55 | lines 153, 187, 196, 205, 212, 216, 320, 341, 477 | User-facing strings: CUJ 1 doubles "Run"; Settings is named three ways; the refusal reuses "never"; the protected-file notice fits one of three report kinds; an internal handler name is shown. | Use the rewrites in the Stage 7 report. | SY-A2, SY-A4, SY-A5, SY-A6, CO-M9, SY-C8 |
| F56 | lines 49, 98, 180, 263, 278, 289, 310, 314, 372, 387, 420 | Ambiguous terms: "run", owner vs user, "today's", four names for a mutating call, "which are kept", "the owner is dead", "may hold only". Results are stated as fact before PT-E2 runs. | Define each term once and use it everywhere. | SY-B2 to SY-B12, SY-B14, SY-C1, SY-C4, SY-C5, SY-C7 |
| F57 | §2 line 357 | A stopped sub-agent's notice is dropped by the child's sink (`src/agent.rs:729-734`). | Say that the closing text returns as the `delegate` result. | CO-M3, Z8-7 |
| F58 | §3.1 line 384; file 10 line 794 | The home-path Seatbelt rules mostly repeat what the profile already denies. | Emit them only when the working folder contains the home path. | ZO-9 |
| F59 | §1-§3, for example lines 301, 383, 413, 459 | The architecture sections drop into implementation detail. | Move the detail to the Detailed Implementation Plan. | ZO-15 |
| F60 | §1.2 lines 269-282 | The effects row has no working folder. | Record it, and refuse a re-run when it differs. | DC-N4 |
| F61 | §3.2 line 398; Non-Goals line 66 | The threat model misses two items: domain fronting (SNI is not bound to the Host header), and the shell under `cli_allow_agent_tools`. | Add both to the threat model. | DC-N8, Z8-12 |
| F62 | §1.4 line 314 | The doc doesn't say why `approvals.running` is not reused. It has no liveness test (`src/store.rs:1100-1105`). | Record the reason. | Z8-13 |
| F63 | lines 261, 263, 292, 354; file 2 line 673 | Small factual errors: the `ToolCtx` count; `content_hash` does exist; `mutates()` includes Forbidden; `delegate` is a tool; `state_survives_serialization` is not relevant here. | Correct each. | CD-N1, CD-N2, CD-N5, CD-N11, CO-N1, CO-N2 |
| F64 | lines 7, 8, 27, 62, 115, 228, 231, 387, 426, 436, 584, 601, 603, 609, 611 | Structure and references: two H3s should be H2s; the Harness2 links don't resolve; some § references are unlinked; t10, HQ2, PR #73, the Axiom proxy, `00-synthesis.md` and the genesis claims are undefined; heading case is mixed; alternatives have uneven shapes; the diagram uses `│` for two meanings; a metadata bullet; CUJs have no goal IDs; OQ4 holds a fixed rule. | Use the fixes in the Stage 7 report. | ST-M9, ST-N10 to ST-N19, CO-N8, SY-C2 |
| F65 | lines 640-866 | Plan wording nits and test renames. | Use the table in the Stage 9 report. | SY9-N1 to SY9-N16 |
| F66 | W6 row line 878 | W6's filter leaves out `tools::fs::` and `mcp::`. | Filter on `tools::`. | CD-N10 |
| F67 | Work units lines 878-881 | W6's `src/**` contains W1-W5, and W7's `docs/**` contains verify's file. This is safe while the rows run in order, but a later fix node can't start while W6 holds `src/**`. | Note it in the run notes. | IR answer to the scope question |

## Dismissed or narrowed

The review swarm is advisory. These proposals were dismissed or narrowed, with the reason:

| Proposal | Decision | Reason |
|---|---|---|
| ZO-1's option: move Harness2's runs outside `~/.heron` | Dismissed | It changes Harness2's layout, which breaches OD8. F01's file-level fix is used instead. |
| ZO-8 / CO-M20's option: limit the toolchain check to macOS | Dismissed | It narrows OD7 on Linux, where bubblewrap also binds `.rustup/toolchains` as writable. |
| DC-M8: keep the last snapshot between runs | Dismissed | It would blame the owner's own edits on Heron. The false claim is corrected instead (F45). |
| DC-M9: rewrite the git rule as a regex, and cap the bytes hashed | Dismissed | The regex conflicts with anchor's tested scan, and a cap that fails open weakens G4 and G5. The `.claude` question goes to an OQ (F19). |
| DC-M11: protect working-folder build files | Protection half dismissed | Protecting build inputs would block the agent's normal work. The routes are named in the threat model instead (F43). |
| DC-M7: seed the repair child across homes | Narrowed | The protect half belongs to Harness2 (F09). |
| DC-B1: pass a full context object to resumers | Narrowed | Replaced by a fixed declared effect plus taint (F02). |
| CO-B2: raw SQL into `approvals` | Narrowed | Replaced by an idempotent second pass (F07). |
| DC-M4: force Ask on identical calls | Moved to an owner OQ | The transcript change stays (F13). |
| DC-M1: structured `execute_call` result | Narrowed | Replaced by a string rule (F10). |
| CO-M23: define skein in the doc | Narrowed | A link is enough (F25). |
| Stage 8's Z8-4 (changes G2) and Z8-5 (A11, contradicts OD7) | Owner calls | Not applied as findings. |

## Traceability matrix

| Raw finding (stage report id) | Consolidated |
|---|---|
| ZO-1, DC-B2, CD-B1, Z8-2 | F01 (and F08) |
| DC-B1 | F02 |
| IR-B1, CD-N9 | F03 |
| CO-M22, IR-B2, IR-M2, CD-M3, CD-M4, SY9-M1 | F04 |
| CO-M24, IR-B3, CD-M1, SY9-M8 | F05 |
| IR-B4, SY9-N2 | F06 |
| CO-B2, SY9-B1, IR-M3, CD-M2 | F07 |
| ZO-3, CO-B1 | F08 |
| ZO-2, CO-M19, DC-M7, Z8-9 | F09 |
| DC-M1, CD-M9, Z8-7 | F10 |
| DC-M5, CD-M21 | F11 |
| CO-M5, DC-M3, CD-M12 | F12 |
| DC-M4 | F13 |
| DC-M6 | F14 |
| DC-M10 | F15 |
| ZO-6, ST-B1, ST-M2, ST-M3, ST-M4, SY-B13, ST-N12 | F16 |
| ST-M5, Z8-5 | F17 |
| ZO-4, CO-M4, CO-M13, SY-B1, SY-B2, CD-M5, CD-M6, CD-M7, SY9-M7, Z8-3, DC-N7 | F18 |
| DC-M9, Z8-11 | F19 |
| ZO-8, CO-M20, CD-M17, IR-M11 | F20, F54 |
| ZO-5, CO-M15, DC-M12 | F21 |
| ZO-7 | F22 |
| CO-M17, IR-M5, CD-M15, DC-N6, SY9-M5, Z8-6 | F23 |
| CD-M16 | F24 |
| IR-M1, CO-M23, CO-N9 | F25 |
| IR-M7, SY9-N9 | F26 |
| IR-M8, CD-M19, DC-N2 | F27 |
| IR-M4, CD-M11, SY9-M2, CO-M7, DC-N3 | F28 |
| CO-N4, CD-M10, IR-N1 | F29 |
| CO-M10, ZO-11, Z8-4 | F30 |
| DC-M2 | F31 |
| CO-M6, SY-C6 | F32 |
| CO-M8, ZO-14 | F33 |
| CD-M14 | F34 |
| CD-M13 | F35 |
| CO-M12, CD-M8 | F36 |
| Z8-1, CO-M11, CO-M14, CD-N6, ZO-10 | F37 |
| CO-N5, IR-M6, CD-M20, SY9-M6 | F38 |
| CO-M16, DC-N5 | F39 |
| CO-M18, SY9-M9, SY9-M10 | F40 |
| CO-M21, ZO-12, SY-C3 | F41 |
| SY-A1, CO-M1, CO-M2, ZO-13, CD-N3, CD-N4, DC-N1 | F42 |
| DC-M11 | F43 |
| Z8-10 | F44 |
| DC-M8, ST-M8 | F45 |
| Z8-8 | F46 |
| CD-M18, IR-M9 | F47 |
| IR-M10, SY9-M3, SY9-M4, SY9-M12, CD-N7, CD-N12, CO-N3 | F48 |
| IR-M12, CO-N7 | F49 |
| CD-N8 | F50 |
| SY9-M11, SY9-M13, SY9-M14, IR-N2, IR-N4, IR-N5 | F51 |
| ST-M6, ST-M7 | F52 |
| SY-A3, CO-N6 | F53 |
| IR-N3, SY9-N1 | F54 |
| SY-A2, SY-A4, SY-A5, SY-A6, CO-M9, SY-C8 | F55 |
| SY-B2 to SY-B12, SY-B14, SY-C1, SY-C4, SY-C5, SY-C7 | F56 |
| CO-M3 | F57 |
| ZO-9 | F58 |
| ZO-15 | F59 |
| DC-N4 | F60 |
| DC-N8, Z8-12 | F61 |
| Z8-13 | F62 |
| CD-N1, CD-N2, CD-N5, CD-N11, CO-N1, CO-N2 | F63 |
| ST-M9, ST-N10 to ST-N19, CO-N8, SY-C2 | F64 |
| SY9-N3 to SY9-N16 | F65 |
| CD-N10 | F66 |
| IR answer to the scope question | F67 |

## Stage reports (same folder)

* `zoom_out_report.md`
* `comprehension_report.md`
* `critic_report.md`
* `intermediate_zoom_out_report.md`
* `implementation_readiness_report.md`
