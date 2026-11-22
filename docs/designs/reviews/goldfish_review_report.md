Status: resolved (auto accept, 2026-11-18). 41 fixes applied to heron-v1.md; #33 contested and skipped. R-1 and the Q10 cap await the owner's sign-off.

# Goldfish Annotated Review Report

**Target:** `docs/designs/heron-v1.md` (`Lines 1-575`, FeatureRange at 91797c0)
**Source findings:** `docs/designs/reviews/critic_report.md` (stage 7: `egm-design`, `egm-structural`, `egm-style`)
**Summary:** 42 findings. PROPOSE FIX: 41. CONTEST: 1 (#33, contractions). Two fixes need an owner sign-off before they're final; each is marked **(owner)**.

## Prioritized Findings & AI Suggestions

### 1. "Always" writes `read_file:*`, opening the whole Ask tier [`egm-design`] - Severity: High (Blocker)
*   **Affected Scope:** Technical Plan §1, check order and verdict table; G2
*   ***AI Suggestion:*** PROPOSE FIX. Confirmed in code: `always_rule()` returns `{tool}:*` for every tool except `shell` (`permissions.rs:353-360`).
*   ***Proposed Edit:***
    *   Ask-tier reads get a new effect, `Effect::OutsideRead`. It isn't high, so allow rules can match it.
    *   `always_rule()` for `read_file`, `search_files`, `list_dir` and `find_files` returns the approved path's parent folder as `<tool>:<parent>/**`, never `*`.
    *   When a read rule is added, and again when rules are loaded, these targets are rejected: `*`, `~`, `~/**`, `/**`, `/Users/**`, `~/Library/**`. At load time a rejected rule is dropped, and Settings shows a one-time notice listing it.
    *   These locations move to Refused: `~/Library/Keychains`, `~/Library/Cookies`, `~/Library/Safari`, `~/Library/Messages`, `~/Library/Mail`, and the Chrome, Brave, Edge and Firefox profile folders under `~/Library/Application Support`.
    *   Tests: `always_on_an_ask_read_is_folder_scoped` and `broad_read_rules_are_rejected`.

### 2. Secret check before Refused makes key stores ask instead of deny [`egm-design`] - Severity: High
*   ***AI Suggestion:*** PROPOSE FIX. Confirmed: `heron.db*`, `.heron/secrets.json` and `credentials.json` all match `sensitive_path()` (`permissions.rs:541`).
*   ***Proposed Edit:***
    *   New check order: Forbidden → deny rules → read-only blocks changes → **Refused (denies)** → secret match (asks) → taint → allow rules → tiers.
    *   The verdict table moves the Refused row above the secret row.
    *   Test: `refused_beats_secret_in_every_mode`.

### 3. Refused misses backups and a relocated `HERON_HOME` [`egm-design`] - Severity: High
*   ***Proposed Edit:*** Refused becomes `cfg.home` as a whole subtree, resolved at runtime, so it covers backups, `secrets.json`, the three databases and any `HERON_HOME`. The individual Heron file entries are removed. The Seatbelt profile denies the same subtree wherever it lives, as `hidden()` already does today (`sandbox.rs:68`).

### 4. The sandbox doesn't enforce the same tiers as `read_file` [`egm-design`] - Severity: High
*   ***Proposed Edit:***
    *   The shell profile is deny-default for reads under `$HOME`, `/Users`, `/Volumes` and `cfg.home`.
    *   It allows reads in the Allowed tier, the toolchain list, and **every folder granted by an allow rule** of the form `read_file:<dir>/**`.
    *   A shell command that names an ungranted Ask-tier path is **denied**, with the hint "use read_file, which can ask", so an approval is never shown and then blocked.
    *   §1's claim about matching tiers is reworded to say exactly this.

### 5. The toolchain list contains credential files [`egm-design`] - Severity: High
*   ***Proposed Edit:***
    *   After the toolchain allows, explicit `(deny file-read* …)` rules cover `~/.cargo/credentials*`, `~/.config/git/credentials`, `~/.npmrc`, `~/.pypirc`, `~/.netrc`, and every path the secret pattern matches inside allowed folders.
    *   The caches narrow to `~/.cache/uv`, `~/.cache/pip`, `~/Library/Caches/Homebrew` and `~/Library/Caches/pip`. U2's build check adds a subfolder only when a build needs it.
    *   Test: `sandbox_cannot_read_cargo_credentials_via_python`.

### 6. No behaviour defined when the sandbox is off or unavailable [`egm-design`] - Severity: High
*   ***Proposed Edit:*** With no active sandbox (`sandbox::active()` is `None`):
    *   CLI providers are refused, with the message "the sandbox is off, so Heron won't run <CLI>".
    *   Shell falls back to the classifier, and Ask-tier reads ask.
    *   The risk notice and Settings show the sandbox status.
    *   `DENIED_HINT` (`sandbox.rs:115`) stops suggesting that the user turn the sandbox off.
    *   Decision-table rows are added for the sandbox-off case.

### 7. The CLI-child profile can't run real installs and drops `DISABLE_*` [`egm-design`] - Severity: High
*   ***Proposed Edit:***
    *   **Per-CLI profile.** It reads the resolved binary's canonical install folder (from `resolve_binary`, `cli_llm.rs:344-378`). It can read and write that CLI's own config and data folders.
    *   **Environment.** The env table adds `DISABLE_AUTOUPDATER=1` and `DISABLE_FEEDBACK_COMMAND=1`, which `child_env` sets today (`cli_llm.rs:1242-1243`).
    *   **Spike checks, UNVERIFIED until run:** Keychain-backed logins, OAuth refresh writes, and Codex's own sandbox nested under Seatbelt.
    *   **Fallback.** A CLI that fails any of these stays experimental.

### 8. `mcp-serve --refuse-all` can't serve the hybrid arm [`egm-design`] - Severity: High
*   ***Proposed Edit:*** `--refuse-all` takes a separate path that never builds `App` or opens the database. It advertises the full `ToolRegistry::builtin()` schemas and refuses every call. Spike precondition: the advertised tool names equal the envelope's tool list.

### 9. The sandbox profile hinges on a noisy measurement [`egm-design`] - Severity: High
*   ***Proposed Edit:***
    *   The security profile is unconditional. A regression is treated as toolchain-list work, never as a reason to revert.
    *   The two measured choices, topology and workspace reads, use 5 runs per task (50 per arm), paired by task.
    *   A change of default needs a lead of at least 4 runs of 50, about 2 standard deviations. Anything less is inconclusive and keeps the current default.

### 10. The workspace-content exfiltration residual isn't recorded [`egm-design`] - Severity: High **(owner)**
*   ***Proposed Edit:***
    *   Add accepted risk **R-1**: "In a conversation with no outside content, injected text in a working-folder file can make Heron send other working-folder content to a new URL without asking."
    *   It counts against the cap of 3.
    *   Add a G7 report-only probe task for this variant.
    *   The owner signs R-1 in `docs/ACCEPTED_RISKS.md`, or asks for the narrow mitigation instead: egress carrying content not typed in the conversation asks after a working-folder read.

### 11. No Risks & Mitigations section [`egm-structural`, `egm-design`] - Severity: High
*   ***Proposed Edit:*** Add an H2 "Risks & Mitigations" between Alternatives and NFRs. Its columns are risk, type, likelihood and impact, mitigation or accepted-risk ID, owner unit, and where it is specified. Rows:
    *   `sandbox-exec` deprecation;
    *   CLI flag drift within a pinned major version;
    *   quota exhaustion during the spike and G7 runs;
    *   orphaned core;
    *   sandbox off;
    *   macOS privacy (TCC) prompts during CUJ 2;
    *   R-1;
    *   autonomous merges into security code;
    *   the UNVERIFIED items: App Sandbox, the Antigravity config path, the Anthropic terms quote.

### 12. A crashed app leaves an orphan core holding `background.lock` [`egm-design`] - Severity: High
*   ***Proposed Edit:***
    *   The core started with `--print-endpoint` exits when stdin reaches EOF or its parent PID changes.
    *   `background.lock` records the holder's PID. On launch, the app terminates a stale `heron-cli serve` holding the lock.
    *   Test: `core_exits_when_app_dies`, within 5 seconds.

### 13. `subscription` detection for opencode and cline isn't fail-safe [`egm-design`] - Severity: High
*   ***Proposed Edit:*** Every CLI provider counts as `subscription = true` unless the user sets `cli_subscription = false` for that provider in Settings, which is the only way to opt out. Test: `unknown_auth_is_held`.

### 14. G4 can't fail, and its counts don't match [`egm-style`, `egm-design`] - Severity: High
*   ***Proposed Edit:*** "claude-cli and at least one other candidate reach Tier 1." Use "the five candidates and the gemini-cli fallback" throughout.

### 15. "Gate" and "secret gate" each mean several things [`egm-style`] - Severity: High
*   ***Proposed Edit:*** Each concept gets one term, defined in the glossary:
    *   "secret check": `sensitive_path()`;
    *   "read tiers": the allowlist;
    *   "decision function": G1;
    *   "release gates": G3, G7 and G8;
    *   "task gate": G7.

    No other uses of "gate" remain.

### 16. "Working folder" vs "workspace" [`egm-style`] - Severity: High
*   ***Proposed Edit:*** "Working folder" means Heron's workspace (the README term). The CLI child's cwd becomes "CLI run folder". Both terms go in the glossary.

### 17. "Spike" names several things [`egm-style`] - Severity: High
*   ***Proposed Edit:***
    *   "Containment check": the per-CLI canary plus the containment tests, in U2.
    *   "Measurement spike": U2s.

    The glossary's "Tier 1" row points at the containment check, and topology (c) is "not measured in v1".

### 18. Subjective claims stated as fact [`egm-style`] - Severity: High
*   ***Proposed Edit:***
    *   Attribute "UI and UX are the weakest part" to the owner.
    *   Replace "known holes" with the finding IDs.
    *   Drop "most CLI wrappers use".
    *   Change "safety claim that defines Heron" to "It breaks G1".
    *   Change "cheap" to "needs no native code".
    *   Use one quotation of the owner.
    *   Introduce T3 Code and Hermes with links from the research note.
    *   Drop "independently".

### 19. Ring size has no rationale, and `/chat` has no slow-consumer rule [`egm-design`] - Severity: Low
*   ***Proposed Edit:***
    *   10,000 events covers a 5-minute disconnect at 30 events per second.
    *   A lagging `/chat` subscriber is disconnected and the run continues. Clients then reconnect through `/events`.
    *   Old clients that don't understand `resync` see the same disconnect they see today.

### 20. `--version` needs an extraction rule [`egm-design`] - Severity: Low
*   ***Proposed Edit:*** The version is the first `\d+\.\d+\.\d+` match, with a test for each candidate CLI's real output.

### 21. Glob expansion is unbounded [`egm-design`] - Severity: Low
*   ***Proposed Edit:*** Expansion is capped at 1,000 entries or 100 ms, and beyond the cap the call asks. The sandbox, not the classifier, closes the gap between check and use.

### 22. `cli_allow_api_key` is global [`egm-design`] - Severity: Low
*   ***Proposed Edit:*** It becomes a per-provider list, for example `cli_allow_api_key = ["gemini-cli"]`. The old `true` means all providers, for compatibility.

### 23. G9 misses resumed conversations and counts refused calls [`egm-design`] - Severity: Low
*   ***Proposed Edit:*** A day counts if it has an `audit` row timestamped that day, with an executed decision, in a session whose origin is `user` or `telegram`.

### 24. The README contract is paraphrased, and G2's change isn't acknowledged [`egm-style`, `egm-design`] - Severity: Low
*   ***Proposed Edit:***
    *   Remove the quotation marks, and cite the mode table at `README.md:121-126`.
    *   Say that Alternative I would make later writes ask.
    *   Add "reads outside the working folder now ask" as a v1 behaviour change in the README, in U6, and in the risk notice.

### 25. Additional Resources and Future Improvements sit under the NFRs [`egm-structural`] - Severity: Low
*   ***Proposed Edit:*** Promote both to H2.

### 26. Q5 and Q7 lack the "Proposed answer" format [`egm-structural`, `egm-style`] - Severity: Low
*   ***Proposed Edit:*** Add bold "Proposed answer." sub-bullets to both.

### 27. `docs/ACCEPTED_RISKS.md` is cited as if it exists [`egm-structural`] - Severity: Low
*   ***Proposed Edit:*** Change "an accepted risk (`docs/ACCEPTED_RISKS.md`)" to "accepted by the owner on 2026-11-18, to be recorded in `docs/ACCEPTED_RISKS.md` (created by U6)".

### 28. Work-unit IDs and anchors are used before they're defined [`egm-structural`, `egm-style`] - Severity: Low
*   ***Proposed Edit:***
    *   Add a glossary row: "U0–U8, U2s, U2w: work units, defined in the Work units table".
    *   Number the root causes RC1 and RC2.
    *   Change "before step 1" to "before U1".

### 29. Lists and fields are inconsistent, and Alternative I isn't a rejection [`egm-structural`] - Severity: Low
*   ***Proposed Edit:***
    *   Use `*` bullets everywhere.
    *   Nest the inline paragraphs in CUJ 5 and the Cons of Alternatives C and E.
    *   Rename Alternative I's field to "Decision".

### 30. The findings table and headings are inconsistent [`egm-structural`] - Severity: Low
*   ***Proposed Edit:***
    *   Move the findings table into its own H2, "Security findings in scope", after The Problem.
    *   Number "Shape of the system" as §0.
    *   Use one suffix form, "(G1, G2)", on every subsection.
    *   Split §1 into H4 subsections, with "Read verdicts" as an anchor.

### 31. No Markdown links [`egm-structural`] - Severity: Low
*   ***Proposed Edit:***
    *   Give the EVE URL `https://`.
    *   Make the resources relative links with full paths.
    *   Link "Technical Plan §1" and the Alternatives to their headings.

### 32. Voice switches to "you" [`egm-style`] - Severity: Low
*   ***Proposed Edit:*** Use "the owner" in Q4, Q8 and Q9.

### 33. Contractions mixed with spelled-out forms [`egm-style`] - Severity: Low
*   ***AI Suggestion:*** **CONTEST.** The two cited lines are inside quoted product messages (CUJ text and the error string), where the wording is the UI's own. Changing them would change the quoted UI copy. Elsewhere the doc consistently uses contractions.

### 34. Repeated synonyms [`egm-style`] - Severity: Low
*   ***Proposed Edit:*** Change "live, real-time" to "real-time", and drop the duplicated pro in Alternative G.

### 35. Filler words and idioms [`egm-style`] - Severity: Low
*   ***Proposed Edit:***
    *   "truly off" → "off";
    *   drop "anyway" and "exactly";
    *   "every location nobody thought of" → "any location not on the list";
    *   "keeps moving" → "changes with each merge";
    *   "are the bar" → "set the requirement".

### 36. Statements repeat across sections [`egm-style`] - Severity: Low
*   ***Proposed Edit:***
    *   State the topology (c) preconditions once, in Alternative A, with cross-references elsewhere.
    *   State the menu-bar keep-alive once, in §4.
    *   State the no-re-taint rule once, in §1.
    *   Keep the Q1 copy, as the zoom-out report asked.

### 37. Ambiguous pronouns [`egm-style`] - Severity: Low
*   ***Proposed Edit:***
    *   Change "These are the friends' own credentials at risk" to "Friends' own CLI credentials are what these gaps expose."
    *   Use "The friend reaches a first reply."
    *   Use "is flagged in the report but doesn't fail the task gate."

### 38. "Forbidden" is undefined [`egm-style`] - Severity: Low
*   ***Proposed Edit:*** Write it as "`Effect::Forbidden` (catastrophic commands, `permissions.rs:98`)".

### 39. The EVE glossary row has install steps [`egm-style`] - Severity: Low
*   ***Proposed Edit:*** Move the install command to the Preconditions, which already carry it.

### 40. "Without copying it" can't be checked [`egm-style`] - Severity: Low
*   ***Proposed Edit:*** Replace it with "no Muse assets, layouts or names are reused".

### 41. Wordy phrasing [`egm-style`] - Severity: Low
*   ***Proposed Edit:*** Use "a release the README labels beta". Split Q1's workspace bullet into an option description and a proposed answer.

### 42. "A few friends" has no scope [`egm-style`] - Severity: Low **(owner)**
*   ***Proposed Edit:*** Add Q10: the research preview is for at most 10 people the owner knows personally, each using their own CLI login. The proposed cap is 10, and the owner may change it.
