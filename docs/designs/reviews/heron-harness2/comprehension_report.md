# Stage 6: Comprehension Test, heron-harness2.md

*   **Reviewer:** `egm-comprehension` (Goldfish)
*   **Target:** the second draft, `02e2ec7`
*   **Status:** resolved in `92de335`

The reviewer found 39 issues: 6 Blockers, 24 Majors and 9 Nits.

## Blockers

| # | Finding | Resolution |
|---|---|---|
| 1 | The repair policy with `mode = "auto"` allows any unmatched command (`permissions.rs:438`) | **Fixed.** It now uses `mode = "ask"` with the existing `DenyAll` approver, plus allow rules for writes to the worktree. The doc cites `allow_rule_covers` for compound commands |
| 2 | H1 can't compile before H2 adds the module and types | **Fixed.** New precondition P-6 adds a skeleton commit: the workspace, empty crates, the `lib.rs` modules and stubs |
| 3 | `mod <name>;` in `registry.rs` resolves to `src/registry/<name>.rs` | **Fixed.** That is now the connector path, everywhere |
| 4 | A redaction hit ended in a public PR | **Fixed.** A redaction hit is now decision-table row 0: never pushed. Added `redaction_hit_never_pushes` |
| 5 | R2 was both a fence verdict and a gate failure | **Fixed.** A table of R2 outcomes, with exact commands and the new-connector exemption |
| 6 | `origin_tainted` fixes were labelled auto-eligible | **Fixed.** `origin_tainted` now forces a "needs owner" verdict, including when a tainted-origin record is attached to a report |

## Majors

| # | Finding | Resolution |
|---|---|---|
| 7 | R7 matching was undefined, and Avatar imports HeronCore | **Fixed.** Avatar is now fenced. R7 tokenises whole identifiers and bans every name declared under HeronCore |
| 8 | R6 could be bypassed with grouped or aliased `use` | **Fixed.** R6 now allowlists `use` paths with groups expanded, bans aliases, and `heron-ext-api` re-exports nothing from `reqwest` |
| 9 | The connector trait couldn't be stored as a trait object | **Fixed.** The exact trait now takes `&self` with `#[async_trait]`. `ExtError` is defined. The macro's expansion is `all() -> Vec<Box<dyn Connector>>` |
| 10 | HQ13 had no hook in `Policy::decide` | **Fixed.** `ext_first_use` is placed after the protective rules. It ignores Yolo and standing rules, and only `once` approvals count |
| 11 | The list of changes to fenced files was incomplete | **Fixed.** §10 is now a complete table of files and the items that change them |
| 12 | MCP `trust` is a bool, and the key's route was unspecified | **Fixed.** The server is registered at runtime with `trust = false`, and the key goes into the spawned child's environment. Added `config_never_contains_sealbox_key` |
| 13 | The reflect path and the `observed`/`observations` names collided | **Fixed.** The jobs make no model call and use an explicit `memory.save` call. The source `observation` is kept apart from `observed`, and the `observations` table is untouched |
| 14 | The tick's gating disagreed across sections | **Fixed.** `Harness2::tick` is now a five-step table, with its gating per step |
| 15 | Inbox notes couldn't carry click actions | **Fixed.** The inbox gains an `action` column, the start route and buttons |
| 16 | Failure-record producers were unspecified | **Fixed.** A producer table, the `origin_tainted` counter, the dedupe key, and a threshold of 2 |
| 17 | The decision table lacked run-failure outcomes | **Fixed.** A run-failure table, with `triggers.state` values |
| 18 | The replay pass was enforced only by the UI | **Fixed.** A `replay` table, which the core enforces. Added `self_healing_refused_without_replay_pass` |
| 19 | The G7 smoke couldn't be run from the doc | **Fixed.** The exact command, binaries, task list and path sets |
| 20 | Plain `cargo test` skips the connector crates | **Fixed.** `--workspace` everywhere |
| 21 | H8's `--locked` check failed before the workspace existed | **Fixed.** P-6 creates the workspace, H8r was removed, and H8 uses `-p` |
| 22 | Forget couldn't find observation suggestions | **Fixed.** `suggestions.origin` and `add_suggestion` arguments, as a `store.rs` change |
| 23 | What switch 6 controls was undefined | **Fixed.** A per-switch table: only Workflow learning registers `serve` |
| 24 | The H5 check failed on a comment, and the plist keys were guessed | **Fixed.** The check now uses `grep -E '^[^#]*--deep'`. The `Info.plist` change was dropped, and prototype P5 checks whether one is needed |
| 25 | §9 pointed at the wrong item | **Fixed.** It now points to the Phase 2 delta |
| 26 | Some named tests lived where their checks couldn't find them | **Fixed.** `record_panic` is now a library function, and the H10 test was renamed to `ingest_refused_while_paused` |
| 27 | Contracts were missing | **Fixed.** New §11 holds the enums, `gate-report.json`, `.heronreport`, the ingest batch, the routes and the config keys |
| 28 | The connector offer's carrier was unspecified | **Fixed.** A new `offer_connector` tool with validated fields. The server checks the session's taint again |
| 29 | R8's data and `check-fence`'s arguments were undefined | **Fixed.** `runs.files_json`, `--repo`, and the absolute binary path |
| 30 | Undefined v1 terms | **Fixed.** Definitions gained a "Terms from v1" block and forward links |

## Nits

| # | Finding | Resolution |
|---|---|---|
| 31 | sealbox: "three" additions versus four | **Fixed.** Four, and one argument format for `prune` |
| 32 | Three PR labels | **Fixed.** All three are listed in one place |
| 33 | Health-check timers | **Fixed.** Both, the 90-second and the 30-second timer, are in §9 |
| 34 | Not every named test is enforced | **Fixed.** `required-tests.txt` lists them, and `verify` runs `require-tests` over it |
| 35 | `publish_report_text` versus the per-report tick | **Fixed.** The config key is the tick's default |
| 36 | R1, R3 and R4 were imprecise | **Fixed.** Each now has a one-sentence definition |
| 37 | CUJ H-5 lacked the tainted-conversation refusal | **Fixed.** Added |
| 38 | Host validation lived in the core | **Fixed.** The rules are listed and move into `heron-ext-api` |
| 39 | Ambiguous environment reference | **Fixed.** The §10 check-7 row is now the single place for it |

## Also folded in

The session building the effect journal, the loop guard and protected paths sent word that these exist. Precondition P-5 records them. Their effects on this design:

*   an interrupted run asks before it re-runs (`interrupted`);
*   `git -c core.hooksPath=/dev/null`;
*   the toolchain hash check before the gate.
