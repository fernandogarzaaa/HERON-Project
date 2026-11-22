# Stage 6 Comprehension Review: heron-v1.md

**Target:** `docs/designs/heron-v1.md` at 8329992. FeatureRange is lines 1-441 and DetailedRange is lines 442-773.
**Reviewer:** `egm-comprehension`, a naive reader working read only against `re-implementation-standard.md`.
**Verdict:** the doc can't be re-implemented as written.
**Count:** 2 Blocker, 25 Major, 3 Nit (30 findings).

The full wording of each finding is in this session's transcript. The table below keeps one line per finding: the gap and the fix direction.

| # | Severity | Finding | Fix direction |
|---|---|---|---|
| C1 | Blocker | There's no verdict table for each read tier across the four modes (readonly, ask, auto, yolo) and taint states. Nothing says what the Ask tier does in Yolo, and paths outside `$HOME` fall into no tier. | Define the modes. Add the full table: {secret, Refused, allow-rule hit, Ask, Allowed} × 4 modes × {clean, tainted}. Give the tier for paths outside `$HOME`. |
| C2 | Blocker | M-1 to M-8 and M-GOV-1 are never defined. M-3 is in U1's scope but never described, M-5's fate isn't stated, the P0 isn't named, and the source branch isn't in this worktree. | Add a findings table: ID, severity, description, code, fixing unit, regression test. |
| C3 | Major | CLI config folders "ask" in G2 but are "Refused" in §1. The test name hedges between the two. | Pick Refused, and align G2 and the test name. |
| C4 | Major | A U1 test asserts sandbox behaviour, but the sandbox change lands in U2. | Split the test: the `read_file` half stays in U1, the sandbox half moves to U2. |
| C5 | Major | Live approvals are answered through `/approve` with a UUID, and parked ones through `/approvals/{id}` with an integer id. The doc mixes the two. | State the route and id for each, and what happens when the 900-second timeout turns a live approval into a parked one. |
| C6 | Major | The `/cancel` key isn't given: `run_id` or `session_id`. Nor is the number of runs allowed per session. | Specify the request body, one active run per session, and the response for a run that has already finished. |
| C7 | Major | The `/events` contract is undefined: the event schema, where the sequence number goes, what happens on eviction, the default `since`, the scope, and the `/chat` filter. | Add an event table, plus rules for eviction and the default. |
| C8 | Major | The `/risk-notice` request and response, the version source and the chat block are unspecified, and "kv store" is never defined. | Specify the shapes, how the version is derived, which entry points enforce the block, and the error returned. |
| C9 | Major | The version-range syntax, the starting ranges, when the check runs, how unparseable output is handled, whether an override persists, and how a logged-out CLI is detected are all missing. | Give the grammar, a table of starting ranges, and each of those behaviours. |
| C10 | Major | The audit hash doesn't say how old rows are told apart from new ones, or what the new columns hold for HTTP turns. | Name the discriminator and give the exact hash input. |
| C11 | Major | Allow-rule migration is ambiguous for broad glob rules, and the one-time notice has no channel. | Define what counts as overlap, the action for each case, and how the notice is delivered. |
| C12 | Major | The hybrid topology is described only with claude-cli flags. The MCP command `heron` may not be on the PATH of a DMG install. | Choose the topology per CLI, limit hybrid to claude-cli, and give the MCP server the absolute path of the running executable. |
| C13 | Major | The spike's decision rule, quality rater, baseline commit and report format are ambiguous. | Compare at the same commit with the flag on and off, and give exact thresholds and the report layout. |
| C14 | Major | The ban on unattended work through a subscription login conflicts with scheduled jobs, and no unit enforces it. | Define "unattended", give the behaviour for each kind of work, and assign a unit and a test. |
| C15 | Major | Keychain is in the upgrade test in one place and out of it in another. The automatic pre-migration backup has no owner. | Align the statements, describe the Keychain fixture, and put the backup in U2. |
| C16 | Major | G9 can't tell owner sessions from background ones, because the `sessions` table has no origin column. | Add an `origin` column in `to_v3` and give the exact criteria. |
| C17 | Major | U6 needs the spike result but depends only on U2. | Make U6 depend on U2w, and finalize the register after G3. |
| C18 | Major | The environment allowlist leaves out credentials for router CLIs and the API-key path (`allow_api_key`). | List the extra variables allowed for each CLI, and make the test assertion precise. |
| C19 | Major | The tiers name locations without giving literal paths, and the CLI-child read list differs from section to section. | Give literal path lists for each tier and each profile. |
| C20 | Major | The tools that serve Outside prompt blocks on demand aren't named, and the `PromptSource` variants aren't listed. | List every variant with its trust class and its tool. |
| C21 | Major | The shell path-resolution rule is underspecified. | Specify the algorithm, with "unresolvable means Ask" as the fallback. |
| C22 | Major | Personalization storage and the avatar's event-to-state mapping are missing. | Say where each field is stored and through which route, and give the transition table. |
| C23 | Major | EVE, testteeth and genesis are never defined, and they, Xcode and a logged-in claude-cli aren't listed as preconditions. | Add one-line definitions and the preconditions. |
| C24 | Major | Tier 1 membership is ambiguous between antigravity-cli and gemini-cli. | Fix the list or give the selection rule. |
| C25 | Major | Q4, Q6, Q7 and Q9 have no proposed answer, and the doc doesn't say whether the proposed answers are what gets built. | Give every question a proposal, and say proposals are built unless the owner overrides them. |
| C26 | Major | The release build's flags, notarization secrets, entitlements and Xcode build are unspecified. | Specify each one. |
| C27 | Nit | The gate-runner commands are incomplete, and no unit check wires in testteeth. | Complete the commands, and add testteeth to the verify unit. |
| C28 | Nit | Vocabulary is used without definition: envelope, the topology letters (a), (b) and (c), Seatbelt, kv store, Option X, and others. | Add a glossary. |
| C29 | Nit | Smaller gaps: store counts differ, `run_parked` is at `app.rs:374`, the doc doesn't say what happens after a restart, Telegram taint, the mechanism that refuses MCP schedules, and the baseline for the risk cap. | Fix each one in place. |
| C30 | Nit | `owner-use.sh` is called a SQL script although it's a shell script, and step 6 passes no arguments. | Fold into the C27 fix. |
