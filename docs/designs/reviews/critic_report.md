# Goldfish Review Report

**Target:** `docs/designs/heron-v1.md` (`Lines 1-575`, from the top through the line before "## Detailed Implementation Plan" at line 576)

**Reviewers:** `egm-design` and `egm-structural` against `feature-spec-standard.md`; `egm-style` against its persona default. One instance each, read only. All three returned reports.

**Count after deduplication:** 1 Blocker, 17 Major, 24 Nit (42 findings).

**Method notes:**
- Findings raised by more than one lens are merged, list every lens, and take the higher severity.
- Earlier reports in `docs/designs/reviews/` were checked. Where a finding touches a fix those reports mark resolved (zoom-out Blocker 1, re-audit N1, comprehension C12, C13, C14), the entry says why the reviewer considers the fix incomplete. No finding repeats a resolved one unchanged.
- The code claims behind the Blocker and these Majors were checked against `src/` in this worktree: `always_rule()` (`permissions.rs:353-360`), `can_always` (`server.rs:408`), `Effect::high()` (`permissions.rs:95-100`), the allow-rule guard (`permissions.rs:251`), the `sensitive_path()` regex (`permissions.rs:541`), `SERVED_TOOLS` and `tools/list` (`mcp.rs:232, 255`), `App::build` in `mcp-serve` (`main.rs:640-643`), `sandbox::active()` and `DENIED_HINT` (`sandbox.rs:53-54, 115`), `hidden()` hiding `cfg.home` (`sandbox.rs:68`), `backup_dir` (`store.rs:277-278`), and the `resolve_binary` paths and `DISABLE_*` variables (`cli_llm.rs:360-365, 1242-1243`).
- Nothing from line 576 on is reported.

## Prioritized Findings (Blockers -> Major -> Nits)

### The "always" option writes `read_file:*`, which opens the whole Ask tier or none of it - Severity: Blocker
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 269, 272, 276-282` (and G2, line 92)
* **Discrepancy Description**: Line 269 says a user grants a folder such as `read_file:~/Projects/**` "from an approval's 'always' option ... That is the existing mechanism". In the code, `always_rule()` returns `format!("{tool}:*")` for every tool except shell (`permissions.rs:353-360`). "Always" is offered only when `!risk.high() && !tainted` (`server.rs:408`), and allow rules are consulted only for non-high effects (`permissions.rs:251`). The doc never says which `Effect` an Ask-tier read gets, and both answers break the design:
    * Non-high effect: one "always" click on any Ask-tier read stores `read_file:*`. That silently opens all of `$HOME`, `~/Library`, other users' folders and `/Volumes`. Only the name regex is left, which is the denylist G2 and Alternative F reject.
    * High effect: no allow rule can ever match, so the "Ask tier with a matching allow rule" row (line 280) can't happen.
    * Line 272 also endorses `read_file:~/**` for Ask-tier paths. Under the current regex, that rule reaches browser cookie stores, `Local Storage` and app tokens under `~/Library/Application Support`. M-4 adds only `Login Data`.
* **Rationale & Recommendation**: G2 is the design's main security claim, and the existing grant mechanism defeats it. Name the Effect for an Ask-tier read. Make the "always" rule for `read_file` and `search_files` path-scoped (the parent folder of the approved path, never `*`). Reject or narrow `*`, `~/**` and `~/Library/**` read rules when they are added and when they are loaded. Either add the sensitive `~/Library` stores (browser profiles, `~/Library/Keychains`, `~/Library/Cookies`) to Refused, or record the residual risk. Add decision-table tests: "always on an Ask read never yields `read_file:*`" and "a broad rule doesn't reach browser profiles".

### The secret check runs before Refused, so Heron's key store and Claude's OAuth file ask instead of being denied - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 270, 272, 278-279` (and G2, line 92)
* **Discrepancy Description**: The check order is "secret match (asks) → Refused (denies)". Several Refused paths also match `sensitive_path()` (`permissions.rs:541`): `~/.heron/heron.db*` (`heron\.db(-wal|-shm)?$`), `~/.heron/secrets.json` (`\.heron/secrets\.json$`) and `~/.claude/.credentials.json` (`credentials(\.json)?$`). They therefore return Ask, while `~/.codex/auth.json`, which the regex doesn't match, returns Deny. An injected "please read X" can get the most sensitive files approved, but not the less sensitive ones.
* **Rationale & Recommendation**: This is a gap in the re-audit N1 fix, not a re-raise. N1 put the secret check first to keep workspace asks, but didn't handle the overlap with Refused. Evaluate Refused before the secret match, or exclude Refused paths from the secret branch. Update line 272 and the table, and add a test that these files are Deny in every mode.

### Refused misses database backups and a relocated `HERON_HOME` - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 270, 286, 553` (and Q3, line 132)
* **Discrepancy Description**: Refused names individual files under a literal `~/.heron`.
    * It misses `<home>/backups` (`store.rs:277-278`). Those snapshots are full database copies with names like `heron-2026-11-13.db`, which neither the Refused globs nor the secret regex match. They fall into the Ask tier, where the Blocker's `read_file:*` opens them. The NFR at line 553 creates one during the v1 upgrade.
    * `HERON_HOME` can move `cfg.home` (`config.rs:292-296`). The literal entries then point at the wrong place. If the new home is outside `$HOME`, the "deny-default under `$HOME`" profile (line 286) doesn't cover it either. Today's sandbox hides all of `cfg.home` (`sandbox.rs:68`), so this is a regression.
* **Rationale & Recommendation**: Make Refused cover `cfg.home` as a whole subtree, resolved at runtime, with named exceptions only where a tool needs one. Apply the same rule in the Seatbelt profile.

### The OS sandbox doesn't enforce the same tiers as `read_file` - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 284-288` (and G2, line 92; CUJ 3, line 180)
* **Discrepancy Description**: Line 284 says the sandboxed shell enforces "the same Refused and Ask tiers at the OS level". It doesn't:
    * The profile is deny-default only "under `$HOME`" (line 286). Ask-tier paths outside `$HOME` (`/Users/<other>`, `/Volumes`, a relocated `HERON_HOME`) stay readable to a sandboxed `python -c`.
    * Allow rules don't reach the profile. A user who grants `read_file:~/Projects/**` still can't run `grep -r` or `git -C` there.
    * Under line 285, a shell command that names an Ask-tier path gets the strictest verdict, Ask. The user approves it, and the sandbox then blocks it, so the approval can never take effect.
* **Rationale & Recommendation**: The zoom-out report marks its Blocker 1 (allowlist only in `read_file`) resolved. The reviewer considers the fix incomplete for the three reasons above. Build the profile's read allowlist from the same roots, including granted allow-rule folders, and make all of `/Users` and `/Volumes` deny-default with explicit allows. Give shell commands that touch ungranted Ask-tier paths Deny with the "use `read_file`" hint, not Ask, or define how an approval widens the profile for that one command. Correct line 284.

### The readable toolchain list contains credential files - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 267, 287, 296`
* **Discrepancy Description**: The fixed list is readable with no secret filtering. `~/.cargo` contains `~/.cargo/credentials.toml`, which line 267 classes as a secret under M-4. `~/.config/git` is git's XDG credential-store location. `~/.cache` and `~/Library/Caches` are broad, and the reviewer marks it UNVERIFIED which specific tools keep tokens there. The shell secret check sees only names on the command line (`names_sensitive_path`, `permissions.rs:483-493`), so in Auto mode a `ProcessExec` that builds the path at runtime reads these files without asking, and so does a CLI child.
* **Rationale & Recommendation**: This reopens part of M-4 at the OS layer. After the toolchain allows, emit explicit `(deny file-read* ...)` rules for every secret-pattern location inside those folders, at least `~/.cargo/credentials*` and `~/.config/git/credentials`. Narrow `~/.cache` and `~/Library/Caches` to the subfolders builds need. Add a sandbox test that reading `~/.cargo/credentials.toml` through `python -c` fails.

### No behaviour is defined for when the sandbox is off or unavailable - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 92, 284-289, 294`
* **Discrepancy Description**: G2's shell half and all CLI-child containment rest on Seatbelt. `sandbox::active()` returns `Kind::None` when `agent.sandbox = "off"`, when `HERON_SANDBOX=off`, or when `sandbox-exec` is unavailable (`sandbox.rs:53-54`). `DENIED_HINT` (`sandbox.rs:115`) tells the model to tell the user they can turn the sandbox off, which an injected page can exploit. With no sandbox, a CLI child (cline-cli runs in yolo mode by default) gets full home reads and network.
* **Rationale & Recommendation**: State the fallback: with no active sandbox, CLI providers are refused, or lose Tier 1 status and show a blocking warning, and shell falls back to the classifier with Ask-tier reads asking. Remove the "turn the sandbox off" advice from `DENIED_HINT`, show sandbox status in the risk notice, and add the case to the decision-table tests.

### The CLI-child profile probably can't run real CLI installs, and drops `DISABLE_*` variables - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 95-99, 101-103, 225, 294-307`
* **Discrepancy Description**: The profile is stated as settled: write only the cwd, read only system folders, the toolchain list, the cwd and the CLI's own config.
    * Verified: `resolve_binary` finds CLIs in `~/.local/bin`, `~/bin`, `~/.npm-global/bin`, `~/.bun/bin`, `~/.volta/bin` and `~/.claude/local` (`cli_llm.rs:344-378`). Only `~/.claude/local` is readable in the profile, and only for claude-cli, so npm, Bun, Volta and `~/.local` installs can't even be exec'd.
    * Verified: `child_env` today sets `DISABLE_AUTOUPDATER=1` and `DISABLE_FEEDBACK_COMMAND=1` (`cli_llm.rs:1242-1243`). The env table at lines 299-305 omits them, so a literal implementation lets claude-cli self-update past its version pin.
    * UNVERIFIED, for the spike: CLIs write to their own config during runs and OAuth refresh, Keychain-backed logins may need `~/Library/Keychains`, and nesting `sandbox-exec` under Codex's own Seatbelt may fail.
* **Rationale & Recommendation**: Make the profile per CLI: read the resolved binary's real install path, read and write the CLI's own config and data folders, and allow Keychain access if needed. Keep the `DISABLE_*` variables in the env table. Add these checks to the G4 containment spike, with a stated fallback such as "the provider stays experimental".

### `mcp-serve --refuse-all` can't serve the hybrid arm as described - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 55, 104, 325-327`
* **Discrepancy Description**: The hybrid arm's server "advertises Heron's tools through its existing `tools/list`". `tools/list` advertises only `SERVED_TOOLS`, 10 memory, task and schedule tools (`mcp.rs:232, 255`), with no `shell`, `read_file` or `web_fetch`. The hybrid arm would therefore see a smaller toolset than the envelope, and the G5 comparison wouldn't measure what it claims to. `heron mcp-serve` also runs `App::build(cfg, workspace)` (`main.rs:643`), which opens `heron.db` and the config under `~/.heron`. As a child of the CLI, it inherits the CLI's Seatbelt profile, where `~/.heron` is unreadable, so it will probably fail at startup.
* **Rationale & Recommendation**: The comprehension report marks C12 resolved, but this mechanism doesn't work. Under `--refuse-all`, advertise the full registry from `ToolRegistry::builtin()` without building a store, or write a static tools-list stub into the CLI's cwd. Add a spike precondition that the advertised tool names match the envelope's tool list.

### The sandbox profile depends on a usability measurement, and the thresholds are within sampling noise - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 289, 328-331` (and G2, line 92; lines 128, 534)
* **Discrepancy Description**: G2 and §1 state the deny-default profile unconditionally. Line 331 keeps it only "if the task gate loses fewer than 2 runs of 30", and doesn't say what ships otherwise. A revert would take the line 284 claim and part of the M-1 and M-4 coverage with it. At a 90% pass rate, 30 binary runs have a standard deviation of about 1.6 runs. So "at least 1 more run of 30" (topology) and "fewer than 2 runs" (workspace reads, sandbox) will be decided by noise on non-deterministic, subscription-backed CLIs.
* **Rationale & Recommendation**: C13 is marked resolved because exact thresholds were added. The reviewer says this doesn't address either problem. Keep the security profile unconditionally, and treat any regression as toolchain-list work. For the two measured choices, use more runs, or paired runs with a stated significance rule, and define the default when the result is inconclusive.

### The workspace-content exfiltration path left open by Q1 isn't recorded as a risk - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 27, 76, 91, 128`
* **Discrepancy Description**: This doesn't challenge the Q1 proposal itself. With workspace reads trusted, the M-1 chain is broken only at the credential read. In Ask mode, an untainted conversation still allows `NetworkEgress` (`permissions.rs:256`), and the seen-URL check applies only after taint (`tools/mod.rs:33-38`). An injected instruction in a workspace file, such as a cloned repo's README, can still have the model read other workspace or Allowed-tier files and send them out in a URL with no prompt. That is M-1's shape with a different payload. The README's known limits (`README.md:293`) mention downloaded files, but the doc neither lists this as an accepted risk nor explains why the G3 reviewer won't rate it P0 or P1.
* **Rationale & Recommendation**: Either add an accepted-risk entry with an ID that counts against the cap of 3, or add a narrow mitigation, such as egress that carries content not seen in the conversation asks after a workspace read of a file Heron didn't write. Add a G7 task for this variant.

### There is no Risks & Mitigations section - Severity: Major
* **Reviewer Lens:** `egm-structural`, `egm-design`
* **Affected Scope:** whole range. The risk material is spread across lines 84, 87, 118, 136, 270, 296, 345, 349-350, 365, 367, 487, 499, 547 and 553-554.
* **Discrepancy Description**: `feature-spec-standard.md` requires a spec to call out its failure modes, security implications, scalability limits and mitigations, and lists "Risks & Mitigations" as a required area. No such heading exists. The material that is present is scattered across Definitions, Open Questions, plan bullets, Alternatives and NFRs, so a reader can't confirm every risk was handled. The UNVERIFIED items (App Sandbox conflict at line 118, Antigravity config path at 270, the Anthropic legal quote at 487) have no single place where their status is tracked. `egm-design` lists risks that appear nowhere:
    * Apple's deprecation of `sandbox-exec`, which G2 and G4 rest on;
    * CLI flag and protocol drift within pinned majors;
    * subscription quota running out during the G7 and spike runs;
    * orphaned core processes;
    * the sandbox-off path;
    * macOS privacy (TCC) prompts attributed to Heron.app during CUJ 2 (UNVERIFIED for CLI children);
    * the workspace exfiltration residual above;
    * autonomous merges into security-critical code, which Q7 covers only in part.
* **Rationale & Recommendation**: Add an H2 "Risks & Mitigations" between "Alternatives Considered" and "Non-Functional Requirements". Give it a table with these columns: risk, type (failure mode, security, scalability or external terms), likelihood and impact, mitigation or accepted-risk ID, owning unit or question, and where it is specified. Include one row per UNVERIFIED item, with how and when it gets verified. The inline text can stay, with the rows pointing back to it.

### A crashed app can leave an unreachable core that holds `background.lock` - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 132, 367, 373`
* **Discrepancy Description**: The core is a child process, and its token exists only in the stdout pipe. The doc doesn't say the core exits on stdin EOF or when its parent dies. If the app crashes or is force-quit, the orphan keeps `background.lock` (`app.rs:660-688`, released only when the process ends), keeps running schedules and possibly Telegram, and can't be reached because its token is gone. The next launch starts a second core that can't own background work and can't show what the orphan is doing.
* **Rationale & Recommendation**: The core exits when its stdin closes or its parent PID changes. On launch, the app detects and terminates a stale core that holds the lock. Add a test: kill the app, and the core exits within N seconds.

### Detecting the `subscription` flag for opencode-cli and cline-cli is undefined and doesn't fail safe - Severity: Major
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 136-137, 316-319`
* **Discrepancy Description**: For opencode-cli and cline-cli, `subscription` is true "when their configured auth is an OAuth login". The doc doesn't say how Heron finds that out: those CLIs' config is Refused to tools, and the formats are vendor-specific. A wrong answer in the permissive direction runs unattended work through a consumer subscription, which is the exposure Q6 exists to prevent.
* **Rationale & Recommendation**: C14 is marked resolved, but its key input is unspecified. Treat every CLI as `subscription = true` unless Heron positively identifies an API-key or local-model route. Document the detection source for each CLI, and test that "unknown" means held.

### G4 can't fail, and its CLI counts don't match the names it lists - Severity: Major
* **Reviewer Lens:** `egm-style`, `egm-design`
* **Affected Scope:** `Lines 94, 101-103`
* **Discrepancy Description**: "pass the containment spike or are labelled experimental" is true in both outcomes, so G4 is met even if every CLI fails. "Tier 1 has at most five CLIs" sets only a ceiling. Line 101 says "The registry already lists all five", but the text names six IDs (claude, opencode, cline, codex, antigravity, plus gemini as the fallback). "Two of them start from a known weak point" counts gemini-cli, which is a conditional fallback.
* **Rationale & Recommendation**: State a minimum, for example "claude-cli and at least one other reach Tier 1". Say "the five candidates and the fallback" or "the six candidates".

### "Gate" and "secret gate" each mean several things - Severity: Major
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 15-17, 31, 38, 59, 91-93, 106, 130, 267, 499`
* **Discrepancy Description**: "Gate" names the decision function (38, 91), the read policy (17, 92), the release criteria (59, 93) and the EVE task run (106, 499). "Secret gate" shifts too: at line 92 it is the whole read policy and is separate from "the existing name-based secret check", while at line 31 it cites `permissions.rs:541`, which is `sensitive_path()`, the thing line 267 calls "the secret check".
* **Rationale & Recommendation**: In a security design, the reader must know which layer each statement is about. Use "secret check" only for `sensitive_path()`, "read tiers" or "read policy" for the allowlist, and "decision function" for G1. Give the release criteria and the task run their own names, or define "gate" in the glossary.

### "Working folder" and "workspace" name the same thing, and "working folder" also names the CLI child's cwd - Severity: Major
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 51, 76, 80, 98, 128, 268, 295-296, 330, 499, 525-528`
* **Discrepancy Description**: Heron's workspace is "workspace" at 51, 76, 80, 128, 330 and 525-528, and "working folder" at 128 and 268. "Working folder" also names `$TMPDIR/heron-cli-<run>` at 98 and 295-296. Line 128 uses both terms for one boundary in consecutive sentences. That boundary decides the Allowed tier, and the glossary defines neither term.
* **Rationale & Recommendation**: Pick "working folder" (the README's term) for Heron's workspace, and give the CLI child's location another name, such as "CLI run folder". Add the term to the glossary.

### "Spike" names several activities, and the "containment spike" is never described - Severity: Major
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 59, 94-99, 104, 119, 291, 320-334, 442, 549`
* **Discrepancy Description**: The doc refers to a "containment spike" (59, 94), a "comparison spike" (104), a "topology spike" (291, 549) and "The spike (U2s)" (328), which measures topology, workspace reads and the sandbox profile. The containment checks (95-99) and the tools-off canary (320-323) appear to be the containment spike, but neither is labelled that way, and the glossary defines "Tier 1 CLI" through the undefined "containment spike". Lines 119 and 442 say topology (c) "may be measured in the spike", which leaves open which spike is meant.
* **Rationale & Recommendation**: Give each activity one name, and point "containment spike" at the canary and containment checks.

### Subjective or unsupported claims are stated as fact - Severity: Major
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 26, 27, 437, 442, 464, 468, 479, 487, 510`
* **Discrepancy Description**:
    * 26: "The UI and UX are the weakest part" is stated as fact, though it is the owner's view.
    * 27: "known holes" is informal.
    * 437: "the pattern most CLI wrappers use" has no source.
    * 442: "the safety claim that defines Heron" is a value judgment.
    * 464: "Rich animation is cheap" doesn't say cheap in what.
    * 468 and 479: the owner's requirement is quoted two different ways.
    * 487: T3 Code and Hermes are cited as precedent but never introduced.
    * 510: "independently recommended" can't be checked from the doc.
* **Rationale & Recommendation**: Attribute line 26 to the owner and replace "known holes" with the finding IDs. Cite or drop 437 and 510. Shorten 442 to "It breaks G1", and say what "cheap" means at 464. Use one quotation of the owner's requirement, introduce or cite T3 Code and Hermes, and drop "independently" at 510.

### The ring-buffer size has no rationale, and `/chat` has no slow-consumer rule - Severity: Nit
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 350, 366`
* **Discrepancy Description**: "10,000 events is enough because a core restart ends all runs anyway" doesn't follow. Ring size depends on disconnect time multiplied by event rate, and `text` events are per chunk. The filtered `/chat` view has no stated lag behaviour. Today a full queue aborts the run (`server.rs:477-491`), and old clients don't understand `resync`.
* **Rationale & Recommendation**: Size the ring from peak events per second and the supported disconnect window, and specify what `/chat` does when its subscriber lags.

### `--version` output needs an extraction rule - Severity: Nit
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 309-313`
* **Discrepancy Description**: `probe_status` keeps the raw trimmed string (`cli_llm.rs:584-588`), and claude-cli's output, for example, carries a product suffix. Since "unparseable" means untested, every user could get the CUJ 5 prompt.
* **Rationale & Recommendation**: Specify the extraction, for example the first `\d+\.\d+\.\d+` token, with a test for each Tier 1 CLI.

### Glob expansion at decision time is unbounded - Severity: Nit
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Line 285`
* **Discrepancy Description**: `cat /**/*` would walk the whole filesystem inside the decision path, and a file can appear between the check and the run.
* **Rationale & Recommendation**: Cap the expansion by entries and time, classify as Ask beyond the cap, and note that the sandbox, not the classifier, closes the time-of-check gap.

### `cli_allow_api_key` is global - Severity: Nit
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 304-307`
* **Discrepancy Description**: Turning it on for gemini-cli also stops the API-key scrub for claude-cli and codex-cli (`cli_llm.rs:1220-1233`, `config.rs:103`). A later provider switch can then move to metered billing without notice.
* **Rationale & Recommendation**: Make it per provider, or say so in the settings text.

### G9's day count misses resumed conversations and counts refused calls - Severity: Nit
* **Reviewer Lens:** `egm-design`
* **Affected Scope:** `Lines 111, 428`
* **Discrepancy Description**: A day counts only for sessions "created that day", so a conversation resumed the next day doesn't count. Audit rows also include refused and denied decisions, which aren't tool calls.
* **Rationale & Recommendation**: Count days that have an executed-decision audit row, timestamped that day, in an owner-origin session.

### The README's "Reading: allowed" contract is paraphrased, and G2's own change to it isn't acknowledged - Severity: Nit
* **Reviewer Lens:** `egm-style`, `egm-design`
* **Affected Scope:** `Lines 128, 281, 533`
* **Discrepancy Description**: The quoted string isn't in the README. It paraphrases the "Reading" column of the mode table (`README.md:121-126`). "Reverses" also overstates Alternative I, which makes later writes ask rather than reads. Meanwhile, G2 itself changes that contract for every read outside the workspace (line 281), and the doc doesn't say so.
* **Rationale & Recommendation**: Drop the quotation marks or quote the table accurately. List the permission-table change as a v1 behaviour change in the release notes and the risk notice.

### Additional Resources and Future Improvements sit under the NFR section - Severity: Nit
* **Reviewer Lens:** `egm-structural`
* **Affected Scope:** `Lines 557-572`
* **Discrepancy Description**: Both are H3 under "## Non-Functional Requirements" (538). The horizontal rule doesn't change the outline.
* **Rationale & Recommendation**: Promote both to H2.

### Q5 and Q7 don't use the "Proposed answer" format that line 149 relies on - Severity: Nit
* **Reviewer Lens:** `egm-structural`, `egm-style`
* **Affected Scope:** `Lines 135, 138, 149` (and 549)
* **Discrepancy Description**: Line 149 says every question except Q8 has a "Proposed answer". Q5 has no such label, and its proposal is written into the question, yet line 549 treats it as settled. Q7's answer is inline plain text rather than the bold sub-bullet. A reader who scans for the label finds two questions without one.
* **Rationale & Recommendation**: Give Q5 a "**Proposed answer.**" sub-bullet stating the full and reduced gate split, and put Q7's answer in the same form.

### `docs/ACCEPTED_RISKS.md` is cited as if it already exists - Severity: Nit
* **Reviewer Lens:** `egm-structural`
* **Affected Scope:** `Lines 84, 110, 134, 411`
* **Discrepancy Description**: Line 84 calls C35-8 "an accepted risk (`docs/ACCEPTED_RISKS.md`)", and line 134 says it "is already accepted". The file isn't in the repo, and C35-8 appears nowhere outside `docs/designs/`.
* **Rationale & Recommendation**: Mark the file as created by this design and name the unit that creates it at line 411. Reword lines 84 and 134 to "to be recorded in", or cite where the acceptance is recorded today.

### Work-unit IDs and other anchors are used without definition - Severity: Nit
* **Reviewer Lens:** `egm-structural`, `egm-style`
* **Affected Scope:** `Lines 15, 72, 76-85, 128, 145, 149, 218, 230, 270, 289, 296, 310, 315, 324, 328, 427, 534`
* **Discrepancy Description**: U0, U1, U2, U2s, U2w and U5 are used throughout, but are defined only after line 576. "Root causes 1 and 2" (15, 230) refer to an unnumbered list (16-17). "Before step 1" (427) could mean the list at line 38 or a unit.
* **Rationale & Recommendation**: Add a glossary row for work units that points to the plan section, number the root causes, and say which list "step 1" belongs to.

### Lists and fields are formatted inconsistently, and Alternative I isn't a rejection - Severity: Nit
* **Reviewer Lens:** `egm-structural`
* **Affected Scope:** `Lines 15-17, 196, 467, 487, 534`
* **Discrepancy Description**: Lines 16-17 are the only `-` bullets. CUJ 5's "Then" and the Cons of Alternatives C and E are inline paragraphs, while the others use nested bullets. Alternative I's "Reason for Rejection" describes a conditional adoption.
* **Rationale & Recommendation**: Use `*` bullets and nested lists throughout. For Alternative I, rename the field "Decision", or move the conditional rule into §2 and keep only the rejection of the default here.

### The findings table, Technical Plan headings and §1 depth don't follow a consistent pattern - Severity: Nit
* **Reviewer Lens:** `egm-structural`
* **Affected Scope:** `Lines 47, 70-87, 200-424, 242-289`
* **Discrepancy Description**:
    * The findings table, which G3 checks against, sits under Definitions.
    * "Shape of the system" is unnumbered, and the goal suffixes in the §1-§7 headings vary in form. §5 and §7 have none.
    * §1 uses bold pseudo-headings and doubly nested tables, so Q2's "verdict table in Technical Plan §1" has no anchor.
* **Rationale & Recommendation**: Move the findings table under The Problem or into its own H2. Use one suffix form, such as "(G1, G2)", on every subsection. Split §1 into H4 subsections.

### The scope has no Markdown links - Severity: Nit
* **Reviewer Lens:** `egm-structural`
* **Affected Scope:** `Lines 63, 119, 130, 137, 523, 552, 559-564`
* **Discrepancy Description**: The EVE URL at line 63 has no scheme. Two research files at line 562 are given without their path. Internal references such as "Technical Plan §1" and "Alternative H" are plain text.
* **Rationale & Recommendation**: Use `https://`, write the resources as relative links with full paths, and anchor-link the internal references.

### The voice switches from third person to "you" - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 133, 145, 146`
* **Discrepancy Description**: Q4, Q8 and Q9 address "you". Everywhere else the doc says "the owner", including Q9's own proposed answer.
* **Rationale & Recommendation**: Use "the owner" throughout.

### Synonyms are repeated - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 122, 504-505`
* **Discrepancy Description**: "live, real-time" says one thing twice. "Visible progress soonest" and "The owner sees the product take shape first" are one pro stated twice.
* **Rationale & Recommendation**: Keep one of each.

### Filler words and idioms - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 95, 350, 373, 497, 508, 541`
* **Discrepancy Description**: "truly off" (95), "anyway" (350), "exactly" (373), "every location nobody thought of" (497), "keeps moving" (508) and "are the bar" (541).
* **Rationale & Recommendation**: Cut the fillers and replace the idioms with literal statements.

### The same statements repeat across sections - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 62; 127 and 266; 119, 442 and 568; 211, 373, 384 and 523`
* **Discrepancy Description**: "/pause" appears twice in one definition. The no-re-taint rule appears twice, the topology (c) preconditions three times, and the menu-bar keep-alive four times.
* **Rationale & Recommendation**: Keep one statement of each, and replace the rest with cross-references. The zoom-out report asked for the Q1 answer to appear in the plan, so keep that copy.

### Contractions are mixed with spelled-out forms - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 165, 196` (compared with 26, 170, 228, 245)
* **Discrepancy Description**: The doc mostly uses contractions but spells some out.
* **Rationale & Recommendation**: Pick one register and apply it throughout.

### Some pronouns and references are ambiguous - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 33, 170, 547`
* **Discrepancy Description**: "These are the friends' own credentials at risk" (33) has no clear antecedent. At 170, "It reaches a first reply" reads as Heron, but the friend is the one acting. "fails the task gate's report, not the gate itself" (547) is unclear.
* **Rationale & Recommendation**: Rewrite each with an explicit subject, for example "is flagged in the report but doesn't fail the gate".

### "Forbidden" appears in the check order with no definition - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Line 272`
* **Discrepancy Description**: "Forbidden" isn't in the glossary or the tier list, so a reader can confuse it with the Refused tier. It is the `Effect::Forbidden` variant (`permissions.rs:98`).
* **Rationale & Recommendation**: Name it as the `Effect::Forbidden` check, or define it in the glossary.

### The EVE glossary row includes install instructions - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Line 63`
* **Discrepancy Description**: `npm ci && npm run build` is a setup step, not part of a definition.
* **Rationale & Recommendation**: Move it to the preconditions or the release checklist.

### "Without copying it" can't be checked - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Line 19`
* **Discrepancy Description**: The line already names what is borrowed from Muse, so the phrase adds nothing measurable.
* **Rationale & Recommendation**: Drop it, or say what is excluded.

### Wordy phrasing - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 34, 128`
* **Discrepancy Description**: "a beta that the README labels as such" (34). The Q1 workspace bullet (128) packs five claims into one paragraph.
* **Rationale & Recommendation**: Write "a release the README labels beta". Split the proposed answer at 128 from the description of the option.

### "A few friends" sets no scope - Severity: Nit
* **Reviewer Lens:** `egm-style`
* **Affected Scope:** `Lines 11, 24`
* **Discrepancy Description**: The release audience has no size, and its size affects support load and the terms-of-service question (Q6).
* **Rationale & Recommendation**: Give a number or a cap, or state that it is deliberately unbounded.
