# Stage 7: Critic Review (design, structural and style), heron-harness2.md

*   **Reviewers:** `egm-design`, `egm-structural` and `egm-style` (Goldfish), working in parallel on lines 1-987 of `92de335`
*   **Status:** resolved in the third draft, `a6cb339`/`a325951`

## Design critic: 2 Blockers, 11 Majors, 6 Nits

| # | Severity | Finding | Resolution |
|---|---|---|---|
| 1 | Blocker | Text the owner pastes or restates is a trusted path to unsandboxed execution, a public push and an install | **Fixed.** Reports now carry a provenance (`typed`, `pasted` or `from_note`). A `pasted` or `from_note` run is never auto-eligible and runs no model code on the Mac (row 3). A new Risks row covers it |
| 2 | Blocker | `public` columns let raw titles, hosts and paths out of the vault | **Fixed.** Agent queries are off by default (HQ23). `title`, `host` and `path` are `aggregate_only` with `row_level_public = false`. `day` is the entity column. G-H9 now says honestly what fixed-job sentences contain |
| 3 | Major | The gate ran with the owner's real `HOME` and credentials | **Fixed.** CI is now the gate's sandbox (`contents: read`, no secrets). The only local model code is the G7 smoke binary, which runs with a disposable `HOME`, no `GH_*` and no `SSH_AUTH_SOCK`. HQ2 states what it can still reach |
| 4 | Major | Alternative B ignored CI, a separate user and a VM | **Fixed.** CI is adopted. A separate user or VM is now Alternative C, kept as the fallback |
| 5 | Major | Pushing diffs that touch `.github/` exposes CI secrets | **Fixed.** Row 0b keeps workflow, script, packaging and eval diffs local until the owner pushes them. HQ24 sets a token without `workflow` scope, no tag pushes, and release secrets in a protected environment. PT6 checks this |
| 6 | Major | The gate wasn't pinned to the fenced commit or to `main`'s copy | **Fixed.** CI runs `main`'s `gate/scripts/verify-selfheal.sh` against the PR head. The smoke test uses fresh worktrees, `git clean -xdf` and the base checkout's runner. A fixed order puts the commit before the checks |
| 7 | Major | Auto-eligible core files had reach the doc didn't account for | **Fixed.** `events.rs` and `embed.rs` are now fenced. R9 limits `schedule.rs`, and R7 now bans process, file and network APIs |
| 8 | Major | R6's token scan could be bypassed | **Fixed** by item 9's redesign: connectors are declarative manifests with no code |
| 9 | Major | The declarative-connector alternative was missing, and the broker was a strawman | **Fixed.** Declarative manifests with a fixed executor are adopted. Rust crates and a broker are now Alternatives I and J, each argued |
| 10 | Major | `Ctx::http` had no rules for redirects or private addresses | **Fixed.** The executor uses `Policy::none()`, the manifest host check, `is_private_ip` after DNS, and auth only to manifest hosts, each with a test |
| 11 | Major | `origin_tainted` missed parsers that run outside a session | **Fixed.** A task-local `OUTSIDE_SCOPE` covers every outside-content handler |
| 12 | Major | HQ5's signing omitted the hardened runtime | **Fixed.** The hardened runtime and the release entitlements are required. A separate revocable identity is offered as an option |
| 13 | Major | sealbox's dependencies float, the key went through the environment, and Risks had gaps | **Fixed.** A hashed lock file and a venv hash check before each spawn. `--key-fd`. New Risks rows for the crash-and-repair loop, disk use, CI secrets, laundering and runner availability |
| 14 | Major | Private text could reach the public diff | **Fixed.** A private-text check using 8-word shingles of the report (row 0b) |
| 15 | Nit | Disabling hooks isn't the whole git attack surface | **Fixed.** `core.fsmonitor`, `diff.external` and `filter.lfs.process` are disabled too, and the `.git` pointer is checked |
| 16 | Nit | `get_limited` doesn't check the first URL | **Fixed.** `check_url` runs first, and the allowlist is checked on the first and final URLs |
| 17 | Nit | `is_outside` must replace both checks; whether these memories can be confirmed | **Fixed.** It replaces both, as a named test. The confirmation rule is stated: owner click, as for v1's email memories |
| 18 | Nit | Alternative G contradicted the effect journal | **Fixed.** Now Alternative H, which separates the publishing journal from Pre-5's journal |
| 19 | Nit | CUJ H-6's path wouldn't be auto-eligible | **Fixed.** The manifest layout is in `connectors/` |
| 20 | Nit | The commit ordering, and `record-replay` writes | **Fixed.** A fixed order is written out. The `record-replay` residual is stated |

## Structural critic: 12 Majors, 10 Nits

| # | Finding | Resolution |
|---|---|---|
| M1 | CUJ H-6's path and file count | **Fixed.** Manifests now: two files |
| M2 | CUJ H-7 had no Workflow learning switch on | **Fixed.** Workflow learning and App use are added |
| M3 | Effect journal: relied on and also rejected | **Fixed.** See design #18 |
| M4 | Future Improvements listed parts that already exist | **Fixed.** Only the completion check and recoverable tool output remain |
| M5 | "Every run ends as a PR" against rows 0 and 4 | **Fixed.** G-H1, G-H4 and the Definitions are qualified |
| M6 | Prototype and precondition IDs collided | **Fixed.** `PT1`-`PT6` for prototypes and `Pre-1`-`Pre-6` for preconditions, both defined |
| M7 | The Open Questions section was missing | **Fixed.** Renamed to `## Open Questions`, grouped |
| M8 | CUJs lacked Given or When | **Fixed.** All 7 have Given, When and Then |
| M9 | Alternatives lacked Pros | **Fixed.** Every alternative has Pros, Cons and Reason |
| M10 | §11 wasn't complete | **Fixed.** Every route and config key is listed |
| M11 | Harness² had no source | **Fixed.** A Definitions entry with its repository and summary |
| M12 | The five properties were duplicated and misattributed | **Fixed.** G-H8 is the only list. §7 refers to it |
| N1-N10 | Diagram alignment, horizontal rules, Status, the question index, Non-Goal wording, unlinked § references, §5 scope, code fences, NFR rollback and security, the Q10 leftover | **Fixed**, apart from N8: the route example is now a table, not JSON. The diagram was redrawn, and section and item links were added and checked by script (0 broken) |

## Style critic: 7 Majors, 5 Nits

| # | Finding | Resolution |
|---|---|---|
| 1 | The Overview described Phase 2 as the default | **Fixed** |
| 2 | "Summaries" contradicted fact 4 | **Fixed.** Now "gated results: counts, aggregates and fixed-job sentences" |
| 3 | Rules credited to OD2 | **Fixed.** They are credited to G-H8 |
| 4 | The P-number collision | **Fixed.** See structural M6 |
| 5 | The effect journal and RepeatGuard were inconsistent | **Fixed** |
| 6 | The auto-install switch was undeclared | **Fixed.** `[harness2] auto_install` exists only in Phase 2 builds, in Definitions and §11 |
| 7 | Harness2, Harness² and APE weren't explained | **Fixed.** Definitions entries, and "Google's Harness²" throughout |
| 8 | "About 27 failures" had no source | **Fixed.** Dated 2026-11-19; PT1 measures it again |
| 9-12 | Subjective wording, jargon, Alternative G's cons, the approval claim | **Fixed.** Measured wording; HQ, kv, delta and DP defined; "Approving this doc approves…" |
