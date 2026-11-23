# Goldfish Review Report: Stage 7, Critic Review

**Target:** `/Users/inan/Documents/PERSONAL/HERON-Project/.claude/worktrees/ultrafix-design/docs/designs/heron-harness-safety.md` (FeatureRange, lines 1-619)
**Reviewers:** `egm-design` (feature-spec-standard), `egm-structural` (feature-spec-standard), `egm-style` (persona only), one instance each.
**Counts as reported:** design 2 Blockers / 12 Majors / 8 Nits; structural 1 / 8 / 10; style 1 / 19 / 8 groups. Overlaps inside this stage are merged below; cross-stage merging is in the consolidated report.

## Design critic: answers to the owner-decision and invariant questions

* **OD1** widened (full args kept, line 284) and has two holes in "never re-run silently" (DC-M3, DC-M4).
* **OD2, OD3, OD8** hold (OD8 collides with Harness2 via DC-B2).
* **OD4** changed in the body (line 359, digits only in failures), and as written the digit rule cannot fire for shell commands at all (DC-M1).
* **OD5** narrowed (rehash only after runs with a journaled call, against that run's own snapshot) and widened (hooks in agent-created repos writable, line 372, against G4). anchor-harness's file-tool `.git` rule is not ported in full (DC-M10).
* **OD6** holds on macOS; `localhost:*` is properly an OQ.
* **OD7** changed (stat-first, lines 465-466); the manifest misses `libexec/` (DC-M6).
* **I1** kept. **I2** kept for tool entries (`src/app.rs:1793-1795`), not for `resume:` (DC-B1). **I3** kept as designed (relies on API-port deny and token not on disk). **I4** kept for tool entries; a resumer can reach memory with no taint available (DC-B1).

## Prioritized Findings (Blockers -> Major -> Nits)

### DC-B1 The `resume:<handler>` path skips the policy check and cannot carry taint - Severity: Blocker
* **Reviewer Lens:** `design`
* **Affected Scope:** §1.5 lines 323, 327-342 (esp. 338-339); §4 line 492
* **Discrepancy Description:** `run_parked` looks up the registry tool, computes its risk, calls `policy.decide`, then re-reads taint (`src/app.rs:1775-1803`, taint 1793-1795). Handling `resume:` "before the registry lookup" skips all of it: read-only mode, "never" rules and current policy are not applied. `ActionIntent` (line 329) has no `tainted` field though line 338 says "tainted from the intent". `rerun(&App, subject)` receives no session, taint or context, so I2 cannot be enforced for a resumer that does tool work.
* **Rationale & Recommendation:** Add `tainted` and a declared `Effect` to `ActionIntent` (or per handler). In `run_parked`, build `Call { tool: "resume:<h>", effect }` and run `policy.decide`; Deny gives `blocked`. Pass a context (session_id, `parked.tainted || kv taint`, origin) to `rerun`, and require resumers to route tool work through `Agent::call_tool` with it.

### DC-B2 Protecting the default `~/.heron` breaks Harness2's repair runs - Severity: Blocker
* **Reviewer Lens:** `design`
* **Affected Scope:** OQ3 line 111; §3.1 line 377
* **Discrepancy Description:** Same as Stage 5 ZO-1. Also: `ReadRoots::build` refuses only `heron_home` and CLI config folders (`src/permissions.rs:935-939`), so line 377's "Both are already Refused" is false.
* **Rationale & Recommendation:** Protect specific files in the default home (`config.toml`, `heron.db*`, `secrets.json`, `journal/`), or exempt `selfheal/runs/`. Settle with Pre-5.

### ST-B1 / SY-B13 A draft decision narrows OD4 without an Open Question - Severity: Blocker (structural), Major (style)
* **Reviewer Lens:** `structural`, `style`
* **Affected Scope:** §2 line 359; G3 line 53; OD4 line 78; rule at line 73
* **Discrepancy Description:** "This draft decides it" limits digit-masking to failures; G3 states the narrowed rule as a goal. No OQ covers it.
* **Rationale & Recommendation:** Add OQ7 "OD4's numbers rule applies to failures only. Proposed: yes, following anchor-harness", with the polling rationale; reword G3 and line 359 to cite it.

### SY-A1 Loop-guard stop notices have no single format - Severity: Blocker (style)
* **Reviewer Lens:** `style` (also `structural` N14)
* **Affected Scope:** lines 172, 179, 357
* **Discrepancy Description:** CUJ 3 "stopped: … results", CUJ 4 "stuck: … identical … same result" (but CUJ 4's outputs differ in timings, line 177). Substituting `<reason>` into line 357 gives nested parentheses and a doubled message.
* **Rationale & Recommendation:** One template in §2, quoted in both CUJs: "Stopped: the same <N> call(s) (<tools>) ran <limit> times in a row with the same results." plus "(failures that differ only in digits count as the same)" when masking applied. Line 357: "…You were stopped. <reason> Without calling any tools, …".

### DC-M1 CUJ 4 and G3's second bullet cannot fire for shell - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** §2 line 358; CUJ 4 lines 175-180
* **Discrepancy Description:** A failing shell command returns `Ok` text ending `[exit code N]` (`src/tools/shell.rs:124-134`); a timeout returns `Ok` (line 107). Under the "starts with `Error:`" rule, `ok` is true, digits are never masked (anchor `repeats.rs:51`), and changing timings defeat the match.
* **Rationale & Recommendation:** Treat non-zero `[exit code N]` and timeouts as failures, preferably via a structured `(ok, text)` from `execute_call`.

### DC-M2 Dismiss can race the `declined` callback - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** §1.5 line 340
* **Discrepancy Description:** Dismiss is unconditional (`src/server.rs:1183-1186`; `resolve_approval` at `src/store.rs:1111-1114` has no status filter). A stale Dismiss after Run again calls `declined` and cancels a just-re-queued repair.
* **Rationale & Recommendation:** Make dismiss a claim (`UPDATE … WHERE id=? AND status='new'`); call `declined` only when it wins.

### DC-M3 Cancel or timeout leaves intents open in a live process - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** §1.3-1.4 lines 296, 307-311, 442
* **Discrepancy Description:** `POST /api/cancel` aborts the run (`src/server.rs:645-652`); timeouts at `src/agent.rs:685` and `src/app.rs:1804` drop the tool future. `guarded` writes no outcome, the live lease keeps the row `intent` with full args for weeks, and it later surfaces as a stale "Run again?". The `watch_end` drop guard would do a blocking rescan inside `Drop` on abort.
* **Rationale & Recommendation:** Drop guard in `guarded` writing `cancelled` (or asking again at once); timeout inside `guarded`; `watch_end` on a spawned blocking thread.

### DC-M4 The model can re-run an interrupted call itself - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** G1 line 49; OD1 line 75
* **Discrepancy Description:** `repair_transcript` fills the gap with "[interrupted before this tool finished; no result]" (`src/agent.rs:272-288`). A user saying "continue" invites the model to re-issue `git commit`; in auto mode it is Allowed with no ask. With Run again also pressed, it runs twice. This is a silent re-run, against OD1.
* **Rationale & Recommendation:** For journaled calls, change the inserted text to say the call may have finished and an approval is waiting. While an ask-again for (session, tool, args_hash) is pending, force Ask on an identical call.

### DC-M5 Local sockets bypass egress and the other rules - Severity: Major (raise to Blocker if a command-running socket is found)
* **Reviewer Lens:** `design`
* **Affected Scope:** §3.2 lines 408, 412; A6 line 539
* **Discrepancy Description:** Connects are allowed to "sockets inside the writable folders", and Heron's writable set includes all of `/private/tmp` and `/private/var/folders` (`src/sandbox.rs:386`). anchor-harness uses a per-run temp folder (anchor `sandbox.rs:9`, 227-231) and denies container, key-agent and `SSH_AUTH_SOCK` sockets (lines 132-151). Any unsandboxed daemon with a socket in `/tmp` or `$TMPDIR` is reachable; one that runs commands defeats egress, protected paths and the toolchain deny. `(allow default)` also leaves mach-lookup open (untested).
* **Rationale & Recommendation:** Allow socket connects only to the workspace and a per-command temp folder (set `TMPDIR` to it); port `default_deny_sockets` including `SSH_AUTH_SOCK`; add a PT-E1 step that enumerates sockets on the owner's Mac and a mach-service fetch check.

### DC-M6 Toolchain manifest misses `libexec/`, and the notice can be skipped - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** §3.4 lines 457, 476-479; G7
* **Discrepancy Description:** `~/.rustup/toolchains/<t>/libexec/rust-analyzer-proc-macro-srv` exists and rust-analyzer runs it unsandboxed (OQ1's threat); changes there never flip to `Changed`. The notice fires only on the after-command transition, so a killed/timed-out command (early return `src/tools/shell.rs:94-108`), a background process or a concurrent command changes the toolchain with no notice. G7's "later command" misses already-running commands.
* **Rationale & Recommendation:** Cover the whole toolchain folder except `lib/rustlib/src` and `share/doc`; notify on any first detection; run the after-check on the timeout path; state the concurrency limit in G7.

### DC-M7 Repair runs keep toolchain and protect state in the throwaway home - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** lines 343, 447-449, 461-463, 481
* **Discrepancy Description:** Same as Stage 5 ZO-2. G5 and G7 fail for the only unattended runs of model-written code (Harness2 HQ2).
* **Rationale & Recommendation:** Seed the child with the main baseline; after the child exits, the main process runs `toolchain::check` and a protect comparison and posts to the main inbox.

### DC-M8 Rehash misses changes between runs; cache key collides - Severity: Major
* **Reviewer Lens:** `design` (also `structural` M8)
* **Affected Scope:** §3.3 lines 436, 441-442, 452; G5 line 55
* **Discrepancy Description:** Each run compares against its own fresh snapshot, so a change after `watch_end` (outliving process, read-only run, owner) is absorbed into the next baseline. Line 452's claim to catch "a process that outlived its command" is false. G5 says "at the start of the run", §3.3 snapshots lazily at the first mutating call. Snapshot keyed by session id collides between a turn and `run_parked`.
* **Rationale & Recommendation:** Persist the last snapshot in kv and compare at every `watch_begin` and `watch_end`; key by run id; align G5's wording.

### DC-M9 Hashing protected folders does not scale; the scan cap fails open - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** lines 368, 387, 436-439, 589
* **Discrepancy Description:** `.claude` at any depth is protected and anchor hashes every file under a protected folder (`protect.rs:166-167`). This repo keeps worktrees in `.claude/worktrees/`, so the first mutating call hashes whole worktrees and the shell cannot write them. At 100,000 entries anchor's scan returns early (`protect.rs:94-97`); repos past the cap get no hooks/config deny.
* **Rationale & Recommendation:** Express in-workspace git protection as a regex needing no scan (`^<ws>/(.*/)?\.git/(hooks(/.*)?|config|modules/.*/(hooks(/.*)?|config))$`); treat truncation as a change; cap bytes hashed; narrow `.claude` to its executable items as anchor's `CLAUDE_HOME_ITEMS` does (`protect.rs:47-48`).

### DC-M10 The file-tool git rule is weaker than anchor's; new repos are a hole - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** §3.1 lines 369-372, 379
* **Discrepancy Description:** anchor's `is_write_protected` refuses any path with a `.git` component (`protect.rs:117-125`). This design protects only hooks/config of existing repos. Open routes: `write_file sub/.git` with `gitdir: ../x` after writing `x/hooks/pre-commit`; `.git/modules/<name>/hooks`; the agent-created repo allowance (line 372), which widens OD5 and contradicts G4.
* **Rationale & Recommendation:** Port the `.git`-component refusal for file tools. In Seatbelt, deny writes under `.git/hooks` at any depth then allow only `\.sample$` names and the folder (last match wins). Or move the new-repo exception into Open Questions.

### DC-M11 Other routes in OQ2's class are not named - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** OQ2 lines 103-106; §3.4 lines 457-460
* **Discrepancy Description:** `src/sandbox.rs:194` makes `.cache`, `.cargo/git`, `.npm` and `Library/Caches` writable; `~/.cargo/git/checkouts` is the same risk as `registry/src`. A workspace `.cargo/config.toml` (`build.rustc-wrapper`) runs the replaced-`rustc` threat through rust-analyzer; `rust-toolchain.toml` may point at a custom toolchain path (unverified).
* **Rationale & Recommendation:** Name `.cargo/git` in OQ2. Protect `rust-toolchain(.toml)` and `.cargo/config(.toml)` at any depth, or state in the threat model that working-folder build inputs (including `build.rs`) are out of scope. Add a PT-T2 check.

### DC-M12 No "what breaks" list for protected paths - Severity: Major
* **Reviewer Lens:** `design`
* **Affected Scope:** §3.1 lines 364-387
* **Discrepancy Description:** Same as Stage 5 ZO-5, plus husky's `prepare` script fails, so `npm install` fails in husky repos.
* **Rationale & Recommendation:** Add the list and the model-facing message for each.

### ST-M2 The draft's own finding sits inside OD3 (and OD7's gloss) - Severity: Major
* **Reviewer Lens:** `structural`
* **Affected Scope:** OD3 line 77; OD7 line 81; §3.4 line 478
* **Discrepancy Description:** "This doc finds it is not cheap" is the draft's judgement in the owner section. OD7's "'Uses' means Heron running cargo or rustc in its shell" may be the draft's gloss; §3.4 then redefines "uses".
* **Rationale & Recommendation:** Keep only the owner's words in OD3/OD7; move findings to OQ or Non-Goals.

### ST-M3 Full-args storage overrides OD1 without an OQ - Severity: Major
* **Reviewer Lens:** `structural`
* **Affected Scope:** §1.2 line 284; OD1 line 75
* **Discrepancy Description / Recommendation:** Add an OQ "Store full args while the intent is open. Proposed: yes."

### ST-M4 "Accepted by OD6" claims more than OD6 says - Severity: Major
* **Reviewer Lens:** `structural`
* **Affected Scope:** §3.2 lines 425-432; OD6 line 80
* **Discrepancy Description:** OD6 does not accept port-443-only, no SSH, no DNS, no Docker, background processes losing network.
* **Rationale & Recommendation:** List the breakage as an OQ with default "accept", or reword to "proposed as accepted (OQn)".

### ST-M5 OQ1 and A11 disagree on what OQ1 proposes - Severity: Major
* **Reviewer Lens:** `structural`
* **Affected Scope:** OQ1 lines 95-102; A11 line 568
* **Discrepancy Description:** OQ1's stronger option ends "The owner decides" with no default; A11 calls it "the recommended change in OQ1".
* **Rationale & Recommendation:** Give the stronger option its own default, or change A11's wording.

### ST-M6 Prototype checks sit inside Open Questions - Severity: Major
* **Reviewer Lens:** `structural`
* **Affected Scope:** lines 126-144
* **Rationale & Recommendation:** Promote to `## Prototype checks` with an anchor; link from Pre-S2 and lines 143-144.

### ST-M7 Risks and mitigations are scattered; residual risks have no mitigation - Severity: Major
* **Reviewer Lens:** `structural` (also `design` standard-compliance note)
* **Affected Scope:** OQ1, OQ2, OQ4, OQ5; §3.2 lines 425-432; Security lines 578-583
* **Rationale & Recommendation:** Add a Risks and Mitigations table (Risk | Source | Mitigation or detector | Residual). Add DC-M5, DC-M9, DC-M11, DC-M12 to it.

### ST-M9 Additional Resources and Future Improvements nest under NFR - Severity: Major (synthesizer: Nit)
* **Reviewer Lens:** `structural`
* **Affected Scope:** lines 601, 611
* **Rationale & Recommendation:** Promote both to H2.

### SY-A2 Interrupted-call summary doubles "Run" - Severity: Major
* **Reviewer Lens:** `style` (also Stage 6 CO-M9)
* **Affected Scope:** lines 16, 75, 153, 320
* **Rationale & Recommendation:** State the shell summary format in §1.5, or change CUJ 1 to "`git commit -m …` was interrupted. Run again?".

### SY-A3 Blocked-host line claims one cause for four refusals; G6 overpromises - Severity: Major
* **Reviewer Lens:** `style` (also Stage 6 CO-N6)
* **Affected Scope:** lines 56, 205, 395-398, 428
* **Discrepancy Description:** Not-allowlisted, non-public address, SNI mismatch and non-CONNECT:443 all get "not on the allowlist". Programs that ignore the proxy produce no line, so G6's "named in the output" overpromises.
* **Rationale & Recommendation:** "[network] blocked <host>:<port>: <reason>. …" with one reason per check; reword G6 to "a connection the proxy refuses is named".

### SY-A4 Settings location named three ways - Severity: Major (synthesizer: Nit)
* **Reviewer Lens:** `style`
* **Affected Scope:** lines 124, 205, 212, 216, 477
* **Rationale & Recommendation:** Use "Settings › Sandbox" everywhere.

### SY-A5 File-tool refusal reuses the word "never" - Severity: Major
* **Reviewer Lens:** `style`
* **Affected Scope:** line 187; I3 line 86; OQ6 line 124
* **Discrepancy Description:** "never" names user rules the app can lift (I3), so a user may expect to lift this one. Labels for other protected categories are undefined.
* **Rationale & Recommendation:** "Refused: <path> is a protected file (it runs code later). Heron's file tools can't change it in any mode." List a label per category in §3.1.

### SY-A6 Protected-file notice fits only one of three report kinds - Severity: Major (synthesizer: P3)
* **Reviewer Lens:** `style`
* **Affected Scope:** lines 196, 443-445
* **Rationale & Recommendation:** "Protected files changed while Heron was working: <paths> (<changed | new | new repository>). …"

### SY-B1 Whether `run_parked` is a run - Severity: Major
* **Reviewer Lens:** `style` (also Stage 6 CO-M13)
* **Affected Scope:** lines 289, 387, 442
* **Rationale & Recommendation:** Resolve line 387's "outside any run" against `parked:<id>` and line 442.

### SY-B2 "Run" undefined at first use and overloaded - Severity: Major
* **Reviewer Lens:** `style`
* **Affected Scope:** lines 49-50, 55, 180, 249, 286-292, 387, 441
* **Rationale & Recommendation:** Define *run* in the Overview; key the snapshot by run id; line 180 "10 successful calls".

### SY-B3 "Repeats 4 times" can mean 4 or 5 - Severity: Major
* **Reviewer Lens:** `style`
* **Affected Scope:** lines 52-53, 170
* **Rationale & Recommendation:** "occurs `max_repeats` times in a row (default 4)" in both G3 bullets.

### SY-B4 to SY-B12, SY-B14 Ambiguous terms - Severity: Major as reported (synthesizer: P3 each)
* **Reviewer Lens:** `style`
* **Affected Scope and fixes:**
  * B4 line 49 "Each one" → "Each call interrupted by a crash".
  * B5 line 278 "which are kept" → "Kept after `args` is cleared".
  * B6 line 310 "the owner is dead" → "that process has exited".
  * B7 line 314 "`unknown` … it is known" → rewrite as suggested.
  * B8 line 372 "may hold only" → "The rehash reports a new repository unless it holds only …".
  * B9 "today's" used for both current rules and pre-change behaviour.
  * B10 "owner" vs "user" interchangeable; define once.
  * B11 four names for a mutating call; define *mutating call* once.
  * B12 "`delegate` is not a tool" → "`delegate` itself is not journaled".
  * B14 line 98 "before the owner's next command" contradicts line 101; line 420 "PT-E2 confirms" → "must confirm".

### DC-N1 to DC-N8 Design nits - Severity: Nit
* **Reviewer Lens:** `design`
* N1 A4 line 527: threshold matches, comparison does not (nudge keys on name:args, `src/agent.rs:559`).
* N2 `max_repeats = 1` stops at the first call (anchor `repeats.rs:60-72`); require >= 2 (line 360).
* N3 Missing lease file = dead owner; lease before first intent commit (lines 307-310).
* N4 Effects row does not record the working folder; `run_parked` uses the current one (`src/agent.rs:301-303`). Record it and refuse a re-run on mismatch.
* N5 Proxy must fail closed. Line 403's "background process loses its network" is false under `localhost:*`: it can reach a later command's proxy.
* N6 kv `server:port` holds one port; a second Heron server goes undenied (line 115).
* N7 Source of the shell's repo list before the first journaled call is undefined (lines 387, 441).
* N8 SNI check does not bind the HTTP `Host` header; note domain fronting via CDN-hosted allowed hosts.

### ST-N10 to ST-N19 Structural nits - Severity: Nit
* **Reviewer Lens:** `structural`
* N10 Relative links to `heron-harness2.md` (lines 7, 62, 609) do not resolve in this worktree; note the branch on each.
* N11 Unlinked/undefined refs: plain `§3.3` (387), `§3.1` (436); `00-synthesis.md` with no path (27, 603); t10 (426); HQ2 (584); Pre-S2/skein (126).
* N12 Other draft parameters with no OQ: `max_repeats` default (360), 30-day retention (285), default host list (420), port 443 only (56), `~/.cargo/bin` excluded (458), trust-on-first-use (462; strongest candidate for Major), no accepted-risk entry (584).
* N13 OQ4 line 115 is a fixed rule inside a question; move to §3.2.
* N14 G8 has no CUJ or plan section; CUJ 1 covers only chat turns of G1's three sources; no goal IDs in CUJs or plan sections.
* N15 §2 has no subsections.
* N16 Mixed heading case.
* N17 A4, A5, A7, A10 lack Pros/Cons.
* N18 Diagram uses `│` for both border and "or" (lines 228, 231).
* N19 Line 8 metadata bullet shape.

### SY-C1 to SY-C8 Style nits - Severity: Nit
* **Reviewer Lens:** `style`
* C1 Vague wording: "when it is sure" (17), "right tool anyway" (426), "friendly half"/"the half that holds" (477-478), "cost far more" (539, 552), unmeasured timings (464, 466).
* C2 Undefined references: Pre-5, Pre-S2, skein, kv, "walks" (323), "fingerprint" (351), "owner budget" (360), t10, HQ2, genesis claims, the Axiom proxy; line 7 "fixes" → "defines".
* C3 Rust tool list differs (lines 21, 81 vs 477); "part" undefined.
* C4 "Refused to the sandbox" (463) misuses a `classify_path` term.
* C5 Filler: lines 339, 344, 142, 301, 462.
* C6 Approval/effect states incomplete after Run again or Dismiss (280, 310-311).
* C7 Background-process network loss given as both reason and breakage (401-403, 431).
* C8 Line 341 shows an internal handler name to the user; "untrusted" vs `Changed` (235, 237); 1.2 GB assumes two toolchains (562); `0.4Xs` (177); "yes and no" (50).

## Verified with no finding (structural)

All in-page anchors resolve under GitHub slug rules. `docs/engineering/SECURITY_THREAT_MODEL.md`, `docs/ACCEPTED_RISKS.md` and `docs/cli-providers.md` exist. Both tables are well formed and all code blocks close. OQ2-OQ6 each have a proposed default. Required feature-spec areas are present; A1-A11 cover U1, U2 and U3.
