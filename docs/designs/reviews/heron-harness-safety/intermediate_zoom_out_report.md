# Goldfish Review Report: Stage 8, Intermediate Zoom Out

**Target:** `/Users/inan/Documents/PERSONAL/HERON-Project/.claude/worktrees/ultrafix-design/docs/designs/heron-harness-safety.md` (lines 1-881, full file; not edited since Stage 5)
**Reviewer:** `egm-zoom-out` (fresh instance), standard `zoom-out-standard.md`, given the Stage 5-7 report paths.
**Harness2 citations** are to the working copy in worktree `serene-shamir-92b5d8` (branch `feat/harness2-1a`), which may differ from the cited revision.

**Verdict:** Taken together, the Stage 6 and 7 fixes would roughly double U1's action path and U3's rehash. About a third of the reported findings trace to five structural choices; changing those removes whole groups of findings. Each proposal is tagged **free** (no owner decision changes) or **needs an OQ** (changes an OD or Goal, so line 73 requires an Open Question).

## Prioritized Findings (Blockers -> Major -> Nits)

### Z8-2 Remove Heron's own home from the protected list - Severity: Blocker (free; OQ3's fourth bullet is a draft proposal)
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** OQ3 line 111; §3.1 line 377; test line 715
* **Discrepancy Description:** This one entry causes ZO-1, ZO-3, CO-B1 and DC-B2 (two Blockers). `heron.db`, WAL and lease files change every run; `config.toml` changes on every Settings save, including the egress edits this design adds. The shell already cannot touch it: the configured home is denied (`src/sandbox.rs:389`) and the default `~/.heron` is not in the writable set (`src/sandbox.rs:386-387`). The only gap is file-tool writes to the default home when `HERON_HOME` is elsewhere, which are Privileged and ask today.
* **Rationale & Recommendation:** Add only the literal files `~/.heron/config.toml` and `~/.heron/heron.db*` to `is_protected`, for file tools only; leave them out of the rehash; do not protect the folder. Harness2's `~/.heron/selfheal/runs/<id>/wt` keeps working. Correct line 377.

### Z8-1 Port anchor-harness `protect.rs` whole, as W2 and W3 do - Severity: Major (free)
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §3.1 lines 364-387; §3.3 lines 434-452; file 4 lines 699-718; W4 line 876
* **Discrepancy Description:** anchor's `protect.rs` already has `scan` with limit and `truncated` flag (82-113), `is_write_protected` (117-125), `apply` (145-160), `Snapshot`/`snapshot` (169-238), `changes` with new-repo rules (308-324), `NEW_REPO_CONFIG_KEYS` and `CLAUDE_HOME_ITEMS`. The doc re-describes it, and Stages 6-7 found gaps the tested code does not have: DC-M10 (anchor refuses any `.git` component), CO-M14 (`changes()` already decides precedence and counts deletions), half of CO-M11, half of ZO-10 (`harness_core::canonical::{content_hash, sha256_file}`). anchor assumes a throwaway per-run workspace; Heron's working folder is long-lived and may hold `.claude/worktrees/**`.
* **Rationale & Recommendation:** Re-label W4 "ported". Cut §3.1/§3.3 down to the differences from anchor (Heron's run boundaries, home paths, Heron's own home). Drop DC-M9's git regex.

### Z8-3 Hold run state in the run frame, not a process-wide map or an `Agent` field - Severity: Major (free)
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §1.2 line 292; §3.1 line 387; §3.3 lines 436-442; file 7 line 753; file 8 line 772
* **Discrepancy Description:** The session-keyed snapshot cache that feeds the shell profile causes ZO-4, CO-M13, SY-B1, DC-N7 and the collision half of DC-M8. The lazy `watch_begin` contradicts G5's "at the start of the run" and misses sub-agent changes before the parent's first mutating call. anchor's `apply` scans per policy build, so the shell can scan per command and drop the cache.
* **Rationale & Recommendation:** Make the snapshot a value owned by the run's frame (depth-0 `run_turn`, `run_parked`, job runner), taken at run start, as `session.tainted` is (`src/agent.rs:545`). Reuse the existing events run id (`crate::events::new_run_id()`, `src/app.rs:1565, 2052, 2251`) as a label. Removes ZO-4, CO-M4, CO-M13, SY-B1, DC-N7, the interior-mutable field and `delegate_child_shares_the_run_id`. Cost: one bounded scan per shell command and one snapshot per depth-0 turn.

### Z8-4 Keep only the transport for Harness2's ask-again - Severity: Major (needs an OQ: changes G2 and the Pre-5 contract)
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §1.5 lines 325-347; §1.2 lines 275, 290; file 1 line 650
* **Discrepancy Description:** Harness2's `triggers.state` already has `running` and `interrupted` (working copy line 846), and its tick runs only inside `own_background_work()` (line 571), the condition under which `recover_interrupted_jobs` (`src/store.rs:1243-1260`) treats `running` as dead. Action rows in `effects` with `begin_action`/`finish`, leases and a `label` with no column duplicate that state (source of CO-M10, ZO-11, part of CO-B2).
* **Rationale & Recommendation:** Reduce the action interface to `journal::ask_again_action(handler, subject, label, session_id, tainted)` plus `Resumer { rerun, declined }`. Harness2 marks `running` as `interrupted` itself and calls it. Update Harness2 in the same change (line 848).

### Z8-5 Settle OQ1 before §3.4 is a fixed interface; run PT-T1 first - Severity: Major (needs the owner: A11 contradicts OD7)
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** OQ1 lines 95-102; PT-T1 line 144; §3.4 lines 454-481; A11 lines 564-568
* **Discrepancy Description:** Line 7 says this doc fixes §3.4, but OQ1's stronger option (A11) deletes most of it (W5, trust API, Settings UI, read-deny, `names_rust_tool`, Harness2's call) and with it DC-M6, CO-M18, half of CO-M19/DC-M7, CO-M20/ZO-8, CO-M21, ZO-12 and the OD7 half of ZO-6. The premise "builds need them" (line 44) is unproven: `writable_extras` (`src/sandbox.rs:194`) makes only `.rustup/toolchains` writable, not `.rustup/downloads`, `.rustup/tmp` or `settings.toml`, so a sandboxed install may already fail today. User cost is comparable: one trust click per `rustup update` (line 480) vs one `rustup toolchain install` per pin bump.
* **Rationale & Recommendation:** Run PT-T1 now; put the cost comparison to the owner with a proposed default for the stronger option (fixes ST-M5). If OD7 stays, adopt ZO-12.

### Z8-6 Drop the Heron API-port deny and kv `server:port` - Severity: Major (free; reword line 497)
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** OQ4 line 115; §3.2 line 407; §4 I3 line 497
* **Discrepancy Description:** Every `/api` route is behind the token check (`src/server.rs:124-137`, layer at 206), and the token is never on disk where the sandbox can read it. The port deny needs a writer that does not exist (CO-M17), and fails for a second server (DC-N6) and in the repair child (ZO-2).
* **Rationale & Recommendation:** Remove it; rest I3's argument on the token and on `config.toml`/`heron.db` being Refused. I3 itself does not weaken.

### Z8-8 Build in two phases so the Pre-5 interfaces do not wait on egress - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** Pre-S2 line 631; W6 line 878; Work units table
* **Discrepancy Description:** A PT-E1 failure stops the whole build including the journal (line 631). W6 wires seven files across `src/**`. U2 does not depend on Pre-5. Harness2 (working copy lines 181, 427) still needs all three before H1.
* **Rationale & Recommendation:** Phase 1: U1, U2, file-tool protected paths, toolchain decision. Phase 2: egress, Seatbelt protect rules, rehash, after PT-E1/E2/P1. Split W6 the same way. Egress as its own PR, same doc.

### Z8-9 "Two interfaces" understates what Harness2 depends on - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** line 7; file 14 line 848
* **Discrepancy Description:** Harness2 also depends on behaviour inside the repair child (egress, protected paths, loop guard, where state lives under a throwaway `HERON_HOME`). ZO-2 and DC-M7 are symptoms.
* **Rationale & Recommendation:** Name these behavioural dependencies in line 7 and file 14, and say which process owns each report.

### Z8-10 Rehashing home paths reports other software's edits as Heron's - Severity: Major (needs an OQ: OD5 says it "hashes the list")
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §3.1 lines 373-376; §3.3; G5; CUJ 6
* **Discrepancy Description:** Installers write `~/Library/LaunchAgents`; `gh auth setup-git` and IDEs write `~/.gitconfig`; nvm, conda and rustup edit rc files. The shell already cannot write these (`src/sandbox.rs:385-388`), and file tools will refuse them, so the home-path rehash is mostly false alarms.
* **Rationale & Recommendation:** Propose in an OQ: rehash workspace paths only; home paths covered by prevention.

### Z8-11 `.claude` at any depth collides with this repository's layout - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §3.1 line 368
* **Discrepancy Description:** With the Heron repo root as the working folder, Seatbelt denies all writes under `.claude/worktrees/**` and the snapshot hashes whole worktrees.
* **Rationale & Recommendation:** Put a narrowing (for example anchor's `CLAUDE_HOME_ITEMS`) to the owner as an OQ, since anchor's `WORKSPACE_NAMES` protects the whole folder (OD5).

### Z8-7 Keep U2's fixes local - Severity: Nit (free)
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §2 lines 351-360
* **Rationale & Recommendation:** DC-M1: a string rule (non-zero `[exit code N]` or shell timeout = failure), not a structured `execute_call` refactor (it is shared by `call_tool`, `delegate` and the concurrent path). ZO-13/CO-M2: count the nudge on name+args from the guard's own history, keeping the test at `src/agent.rs:1361`. CO-M3: a stopped sub-agent's closing text already returns as the `delegate` result; say so.

### Z8-12 CLI-provider non-goal does not cover `cli_allow_agent_tools` - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** Non-Goals line 66
* **Discrepancy Description:** With `cli_allow_agent_tools = true`, cline-cli and goose-cli run their own shell loop with open network under the Cli profile (`docs/cli-providers.md:102-109`). Protected paths are safe there (Cli profile writes only its run folder, `src/sandbox.rs:402`).
* **Rationale & Recommendation:** State it in the threat model.

### Z8-13 Record why the parked path cannot just reuse `approvals.running` - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §1.4 line 314
* **Discrepancy Description:** `recover_interrupted_approvals` (`src/store.rs:1100-1105`) has no liveness test, and `run_parked` runs in whichever process serves the API (`src/server.rs:1177`), so that route could run a parked call twice. The lease is needed.
* **Rationale & Recommendation:** Record the reason in the doc.

## Verdict on earlier proposed fixes (Q1)

| Finding | Verdict | Why |
|---|---|---|
| CO-B2 | Simplify | No raw SQL into `approvals` inside the transaction (it would copy `park_approval`'s dedupe, `src/store.rs:1062`). Use an idempotent second pass: any `interrupted` row with no `approval_id` gets one. Keep recovery on the tick. |
| DC-B1 | Simplify | One fixed declared effect for `resume:` (`ProcessExec`, matching line 338's `"exec"`), and `policy.decide` with the taint `run_parked` already computes (`src/app.rs:1793-1795`). Reject the context object: Harness2's resumer only re-queues. |
| DC-M2 | Keep | Claim-style dismiss mirrors `claim_approval` (`src/store.rs:1094-1096`). |
| DC-M3 | Keep, narrowed | Drop guard writes `cancelled`; no ask-again for a user cancel; comparison on a blocking thread. |
| DC-M4 | Simplify | Keep the `repair_transcript` text change; put "force Ask while an ask-again is pending" in an OQ. |
| DC-M5 | Keep, by reuse | Port anchor's per-run temp folder and `default_deny_sockets`. |
| DC-M6 | Moot under A11 | Otherwise cover the whole toolchain folder minus `lib/rustlib/src`. |
| DC-M7 | Split | Toolchain half moot under A11; protect half belongs to Harness2 (OD8), which calls `protect::snapshot`/`changes` around the child. |
| DC-M8 | Reject the stored baseline | A baseline across runs blames the owner's own edits on Heron. Correct line 452 instead. |
| DC-M9 | Reject regex and byte caps; route `.claude` to an OQ | Regex conflicts with the tested scan; a fail-open cap weakens G4/G5 unless a cap hit counts as a change. |
| DC-M10 | Keep | Comes with the port. |
| DC-M11 | Reject the protection half | Protecting workspace `.cargo/config.toml`/`rust-toolchain.toml` starts protecting build inputs (`build.rs`, `Cargo.toml`, `package.json`), which is the agent's normal work. Draw the line in the threat model; keep "name `.cargo/git` in OQ2". |
| ZO-11 | Superseded | By Z8-4. |
| ZO-12 | Keep | Keep the before-check (the profile needs it); drop only the parser. |
| CO-M16 | Keep | One sentence: the proxy fails closed. |
| ST-M7 | Keep, as a replacement | The risks table replaces the scattered lists. |

Cheap spec fixes to keep: ZO-5/CO-M15/DC-M12, ZO-9, ZO-10, CO-M5 to CO-M7, CO-M22, CO-M24, SY-A1, DC-N2 to DC-N5. Link-only: CO-M23, CO-N8.

## Proposed fixes that would weaken OD1-OD8 or I1-I4 (Q4)

| Source | What it weakens | Direction |
|---|---|---|
| ZO-1 option "move Harness2 runs outside `~/.heron`" | OD8 (changes Harness2's layout) | Use Z8-2. |
| ZO-8/CO-M20 option "limit the toolchain check to macOS" | Narrows OD7 on Linux; bubblewrap binds `.rustup/toolchains` writable (`src/sandbox.rs:441-442`) | Reword G8 instead. |
| DC-M9 narrowing `.claude` | Narrows OD5 (anchor `protect.rs:29` protects all of `.claude`) | Needs an OQ. |
| DC-M9 caps without "cap hit = change" | G4, G5 | Reject, or report a cap hit. |
| ZO-12 read as dropping the before-check | OD7 ("before … refuses") | Keep the before-check. |
| `max_repeats = 0`, `egress = "open"` | Can switch off OD4's stop and OD6's allowlist | Need OQ entries. |
| Z8-5 (A11) | Contradicts OD7 | Owner decision only. |
| Z8-4 | Changes G2 and the Pre-5 contract | Owner decision only. |
| Z8-6 | Changes only the I3 argument | Reword line 497. |

No proposed fix weakens I1, I2 or I4.
