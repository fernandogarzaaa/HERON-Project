# Goldfish Review Report: Stage 5, Initial Zoom Out

**Target:** `/Users/inan/Documents/PERSONAL/HERON-Project/.claude/worktrees/ultrafix-design/docs/designs/heron-harness-safety.md` (lines 1-881, full file)
**Reviewer:** `egm-zoom-out` (one instance), standard `zoom-out-standard.md`
**Context given:** the target doc, the Heron codebase (read-only), anchor-harness port sources, and a working copy of heron-harness2.md from worktree `serene-shamir-92b5d8`. That worktree is on branch `feat/harness2-1a`, not `docs/heron-harness2` (`e1ecd19`), so its text may differ from the revision this doc cites.

Severity mapping used across this review: Blocker = P0/P1 (decided per finding in the consolidated report), Major = P1/P2, Nit = P3.

Context sufficiency: passes. Overview, Problem, Goals and Owner decisions (lines 12-89) state the objective and motivation.

## Prioritized Findings (Blockers -> Major -> Nits)

### ZO-1 Protecting the default `~/.heron` locks Harness2's repair out of its own worktree - Severity: Blocker
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** OQ3 line 111; §3.1 line 377; file 4 lines 705, 715; §3.1 line 384
* **Discrepancy Description:** OQ3 protects the default `~/.heron` even when `HERON_HOME` is elsewhere. Harness2 runs repairs with `HERON_HOME=~/.heron/selfheal/runs/<id>/home` and the worktree at `~/.heron/selfheal/runs/<id>/wt` (`heron-harness2.md:420-421`). Every `write_file`/`edit_file` in the repair worktree would then be Refused by `is_protected`. If "the home paths" at line 384 includes `~/.heron` in the Seatbelt write-denies, `cargo test` cannot write `target/` either. Line 377's claim that both homes are "already Refused for reads and writes" is also wrong: `ReadRoots::build` refuses only the configured home (`src/permissions.rs:949`).
* **Rationale & Recommendation:** Protect only `~/.heron/config.toml` and `~/.heron/heron.db` (files, not the folder), or exempt the working folder and its parents as `hidden()` does (`src/sandbox.rs:200-201`), or move Harness2 runs outside `~/.heron`. Correct line 377.

### ZO-2 Per-home state does not survive the disposable repair home - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §1.5 line 343; §3.4 lines 461-462, 481; OQ4 line 115; §4 I3 line 497
* **Discrepancy Description:** The repair child keeps its toolchain baseline, protect reports, inbox and kv `server:port` in its throwaway `heron.db`. The child makes its own trust-on-first-use baseline from whatever toolchain it starts with, so a change during the repair is reported only to an inbox that is deleted. Line 481 ("the refusal and the Seatbelt deny cover them too") does not hold for a change made mid-repair. The "API port is always denied" rule reads `server:port` from the child's own database, not the main core's port (not verified end to end).
* **Rationale & Recommendation:** Keep the toolchain baseline and the server port in one machine-wide place (for example the default home), or pass them to the child. Route reports to the main inbox.

### ZO-3 Rehash of Heron's own home has no exclusions and no real cost estimate - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §3.1 line 377; §3.3 lines 436-441; Performance line 589
* **Discrepancy Description:** §3.3 hashes "each file in a protected folder", and Heron's home is a protected folder. It holds files Heron changes in every run: `heron.db-wal` (checkpointed every 6 hours, `src/app.rs:2120-2129`), this doc's lease files in `<home>/journal/` (line 246), and Harness2 worktrees of 3-5 GB each (`heron-harness2.md:1018`). Either G5 fires on Heron's own writes, or each run hashes gigabytes twice; "milliseconds" (line 589) does not hold.
* **Rationale & Recommendation:** List exactly which Heron-home files are compared (likely `config.toml` only) and state that the database, WAL, leases and runs are excluded.

### ZO-4 Run state is placed on the wrong objects - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §1.2 line 292; §3.1 line 387; §3.3 line 441; file 7 line 753; file 8 line 772
* **Discrepancy Description:** `run_id` becomes a field on `Agent`, but `run_turn` takes `&self` (`src/agent.rs:468`) and a stored agent is reused across workflow steps (`src/app.rs:2253`). The protect watch is keyed by session id, so a `run_parked` in a session with a live turn ends that turn's watch with its own `watch_end`. The stated reason (avoid editing 32 `ToolCtx` literals) is a code convenience driving architecture.
* **Rationale & Recommendation:** Carry the run id in `ToolCtx` (built per turn, `src/agent.rs:544`) with a default. Key the watch by run id.

### ZO-5 No "what breaks" list for protected paths - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §3.2 lines 425-432; OQ3 lines 107-111
* **Discrepancy Description:** Denying writes to `.git/config` breaks `git remote add`, `git push -u`, `git branch --set-upstream-to`, `git config user.email` and `git submodule init` in the shell. Protecting `.claude` at any depth makes this repo's own `.claude/worktrees/**` read-only when the repo root is the working folder. `.vscode`/`.idea` edits and "add this to my .envrc" are refused.
* **Rationale & Recommendation:** Add a "What breaks" list for protected paths beside OQ3, so the owner can weigh the OQ3 default.

### ZO-6 Draft decisions are made outside Open Questions, against line 73 - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** line 73; lines 263, 284, 359, 360, 414-419, 436, 465-466, G7 line 57
* **Discrepancy Description:** Line 73 says draft defaults live only in Open Questions. The body decides these without an OQ entry:
  * OD1 widened: full arguments kept (line 284) where the owner said "args hash".
  * OD2 interpreted: `NetworkEgress` treated as a read (line 263), though `src/permissions.rs:36-38` describes it as sending model-composed data outside.
  * OD4 narrowed: digits ignored only in failures (line 359, "This draft decides it"); G3 and CUJ 4 build on it.
  * OD4 widened: `max_repeats = 0` turns off the stop OD4 requires (line 360).
  * OD5 narrowed: G5 and line 436 rehash only after runs that changed something; OD5 says "after each run".
  * OD6 widened: `egress = "open"` (lines 414-419, 596); OQ6 only settles who may change the list.
  * OD7 reinterpreted: stat-first then hash (lines 465-466) in place of "hash-checks"; G7 and line 478 redefine "uses" from "Heron running cargo or rustc" to "any route, enforced by Seatbelt", which is OQ1's second addition stated as a Goal.
  * OQ3's default (`~/.heron` protection) is built into the plan (line 377, test at line 715).
* **Rationale & Recommendation:** Move each into an OQ entry with a proposed default, or remove it.

### ZO-7 OD5 is contingent on OQ3 - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** OD5 line 79; OQ3 lines 107-111
* **Discrepancy Description:** OD5 says "anchor-harness's list". That list already includes `.gitmodules`, `.husky`, `.claude`, `.vscode`, `.idea`, `.ripgreprc` and git `config` (`anchor-harness/crates/harness-runtime/src/protect.rs:25-43`). OQ3 presents these as "more than the owner listed". If the owner rejects OQ3, OD5 as written goes unmet. LaunchAgents, which OD5 names, is not in anchor-harness's runtime list.
* **Rationale & Recommendation:** Reframe: the anchor-harness names are OD5 itself; OQ3 covers only genuinely new names (`~/.gitconfig`, `~/.cargo/config.toml`, default `~/.heron`).

### ZO-8 G8 contradicted by the toolchain refusal on Linux and Windows - Severity: Major
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** G8 line 58; line 823; line 216; line 67
* **Discrepancy Description:** G8 says Linux and Windows "build and behave as today". Line 823 applies the toolchain refusal there, and the only re-trust path is the Mac app's Settings › Sandbox. File-tool protection (line 67) also changes behaviour on every platform. OD8 is kept literally; G8 is not.
* **Rationale & Recommendation:** Reword G8 to name the cross-platform changes, or limit the toolchain check to macOS.

### ZO-9 Most Seatbelt home-path rules add nothing - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §3.1 line 384; file 10 line 794
* **Discrepancy Description:** The shell profile already denies writes outside tmp, the working folder and `writable_extras` (`src/sandbox.rs:385-388`), and denies Heron's home (`src/sandbox.rs:389`). Home rc files, `.gitconfig`, `.cargo/config.toml` and LaunchAgents are already unwritable unless the working folder contains them.
* **Rationale & Recommendation:** Emit home-path rules only when the working folder is the home folder or a parent of these paths, and say so.

### ZO-10 Duplicate hashing and scanning helpers - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** lines 354, 655, 672, 387
* **Discrepancy Description:** Line 354 says Heron has no `content_hash`; one exists at `src/tools/fs.rs:63` (bytes, not canonical JSON). The doc adds two canonical-JSON SHA-256 helpers (`args_hash`, the guard fingerprint). The bounded scan could reuse `ignore::WalkBuilder` (`src/tools/fs.rs:815-818`, `src/ambient.rs:19-22`) with hidden/gitignore filters off. anchor-harness `effects.rs:113-184` already has the begin/commit/mark-unknown shape; the doc does not say why it is not ported.
* **Rationale & Recommendation:** One shared canonical-hash helper; reuse the walker; state why `effects.rs` is not ported.

### ZO-11 The resumer registry is more general than its one caller needs - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §1.5 lines 325-342
* **Discrepancy Description:** A global `register_resumer` registry with a trait serves one caller. Harness2 records actions from the process holding `background.lock`, so actions need no leases and could follow `recover_interrupted_jobs`.
* **Rationale & Recommendation:** Keep one ask-again path; consider a simpler direct hook.

### ZO-12 The toolchain rule is enforced twice - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §3.4 lines 477-478
* **Discrepancy Description:** A command-line parser and a Seatbelt read-deny enforce the same rule; the parser is the weaker one.
* **Rationale & Recommendation:** Rely on the deny and turn its failure into the friendly message after the command.

### ZO-13 "Reproduces the nudge exactly" is not true - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** line 356; A4 line 527; CUJ 3 line 172
* **Discrepancy Description:** Today's nudge compares name and raw arguments only (`src/agent.rs:559-561`). `count()` also compares success and output (`repeats.rs:52`), so three identical calls with changing output are no longer nudged. CUJ 3 says "stopped:", the ported text says "stuck:" (`repeats.rs:76`).
* **Rationale & Recommendation:** Correct the claim or keep a name+args count for the nudge; align the notice text.

### ZO-14 "The user is told once" fails for crashed jobs - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** line 314; G1 line 49
* **Discrepancy Description:** A crashed job gets an inbox note from `recover_interrupted_jobs` and, separately, one ask-again approval per interrupted call.
* **Rationale & Recommendation:** State the combined behaviour or suppress one.

### ZO-15 Technical Plan drops into implementation detail - Severity: Nit
* **Reviewer Lens:** `zoom-out`
* **Affected Scope:** §1-§3 (for example lines 301, 383, 459, 413)
* **Discrepancy Description:** SQLite `synchronous` mode, Seatbelt regex syntax, stat fields and env variable lists sit in the architecture sections.
* **Rationale & Recommendation:** Keep §1-§3 at interfaces; move detail to the Detailed Implementation Plan.

## Owner decisions and invariants (reviewer's answer)

* **OD3** kept (lines 63, 77). **OD8** kept literally; G8 is not (ZO-8).
* **OD1, OD2, OD4, OD5, OD6, OD7** changed in the body outside Open Questions (ZO-6, ZO-7).
* **I1** kept: no route turns the sandbox off; `open` only drops network rules.
* **I2** kept: `ctx.read_grants()` drops conversation grants once tainted (`src/tools/mod.rs:42-50`); `run_parked` re-reads taint for `shell` (`src/app.rs:1793-1795`).
* **I3** kept by argument; the API-port leg likely fails in the repair child (ZO-2), the token leg still holds. OQ6's "only through the API" sits awkwardly with the settings living in `config.toml` (line 414), though the agent cannot write that file.
* **I4** kept (lines 500-501).

## Port claims checked

`repeats.rs` is 204 lines with six tests, `count()`, `mask_digits` and the stated limits. `proxy.rs` allows port 443 only with public-address and SNI checks. anchor-harness `sandbox.rs:217-237` has `(deny network*)`, `localhost:{port}`, mDNSResponder and SSH-agent rules. `AlreadyCommitted` exists.
