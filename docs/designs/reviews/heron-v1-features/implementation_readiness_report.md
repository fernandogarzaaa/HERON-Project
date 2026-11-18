# Stages 8 and 9: zoom-out re-check and implementation readiness

Status: resolved

*   **Target:** `docs/designs/heron-v1-features.md` at commit `78f55d6`.
*   **Order:** stage 8 (full file) and stage 9 (the Detailed Implementation Plan) ran at the same time, not one after the other, to save time. Their findings were resolved together.
*   **Reviewers, each in a fresh context:**
    *   stage 8: `egm-zoom-out`;
    *   stage 9: `egm-implementation-readiness`, `egm-codebase-details` and `egm-style`.
*   **Results:**

    | Reviewer | Verdict | Blockers | Majors | Nits |
    |---|---|---|---|---|
    | zoom-out | — | 0 | 2 | 6 |
    | implementation readiness | not ready | 4 | 13 | 11 |
    | codebase details | not ready | 4 | 15 | 7 |
    | style | — | 0 | 5 | 10 |

*   **Resolution:** every finding was accepted. Several overlap; they are merged below.

## What changed in the design

| Finding (merged) | Fix |
|---|---|
| Eval format grew into an HTTP scripting layer (zoom-out M1); ft3 can't answer its approval after dropping the stream (readiness B2, codebase B-2); ft4 races the run record (readiness M6, codebase M-8); ft6's record check can't fail (codebase B-4) | `drop_stream`, `after`, `wait_sql`, templating, `prompt: null` and `http_json` are removed. F3 and F4 add no tools, so their behaviour is covered by Rust and integration tests (heron-v1.md §5 allows "an existing task covers it"). ft6 is a normal prompted task |
| Two switches and three enforcement points (zoom-out M2) | A core key filters the registry's tool names once per turn, and the agent, `save_automation` and the workflow checker all read that filtered list. F2 has one key; F6 has no key (the app's schedule is its switch) |
| Checkpoint ids are reused, so id ranges can restore another run's files (codebase B-1) | A run record keeps a snapshot of each checkpoint (`id`, `path`, `after_hash`, `ts`). Undo restores a checkpoint only if its row still matches. "Restore anyway" uses today's confirmation string. `heron undo --run` is dropped |
| Swift paths contradict v1's package layout (readiness B1, codebase B-3) | Paths follow v1: views in `HeronUI`, client and models in `HeronCore`, the App Intent in `Heron`, tests in `HeronUITests` and `HeronCoreTests`. The fallback edit to this doc is gone |
| Fake binaries never wired; ft5b's key off; fakes print to stdout (readiness B3, codebase M-6, M-7) | New task key `bins`; a fixture layout with `work/`, `web/` and `bin/`; fakes write to `--output-path` and handle `list`; ft5b turns `shortcuts` on |
| `git merge-base` in the gate (readiness B4) | `BASE` is read from a committed `eval/features/base.json`, as `verify-v1.sh` reads `heron_commit`. The owner writes it when cutting each branch |
| Missing files: `store.rs`, `agent.rs`, `personal.rs`, `server.rs`, `lib.rs`, `tests/**` (codebase M-1, readiness M9) | Added to the item lists. New `t` units (`tests/**`) in B2, F2, F3, F4 and F5 re-record the API fixtures and add integration tests |
| "Once only" leaks through `*` rules, `set_tool` and grant sites (codebase M-2, readiness M11) | One function, `permissions::once_only(tool)`, checked at the allow-rule step, by `may_grant_for_conversation`, at every `can_always` site (`server.rs`, `main.rs`, `telegram.rs`) and in `set_tool_setting`. Named tests |
| F6 counts don't match `/api/today`; wording unspecified (codebase M-3, readiness M16) | Window, queries, exact phrases and plurals given. ft6's seed rows named |
| Chat context and session titles or auto-memory (codebase M-4) | The title comes from the typed message, or "Asked about `kind` from `app`". `learn` gets the typed message only |
| `mark_tainted` and `notify()` signatures (codebase M-5) | Both are `App` methods with the bus and the in-memory flag. Signatures are in the plan |
| F5 timeout against `tool_timeout_secs` (codebase M-9) | F5 uses the smaller of 110 s and `tool_timeout_secs` minus 5 s, spawns with kill-on-drop, and reuses `cli_llm.rs`'s environment, temp-folder and process-group helpers |
| `shortcuts list` argv not listed (codebase M-10) | Added to the process table |
| Gate weaker than `verify` on the Slint app and the bundle (readiness M8, codebase M-11) | Every `src/**` check and the gate run `cargo test --locked --features desktop --bin heron-desktop`; the gate runs `build-app.sh --unsigned` |
| The threat-model sections don't exist (codebase M-12) | B5 adds the section "Ways content enters, and processes Heron starts"; each feature's `d` unit adds its exact line, checked by `grep -qx` |
| `set_feature` bypasses the live config (codebase M-13) | It goes through `App::update_config` |
| NSServices entry underspecified (codebase M-14, readiness M12) | The full dictionary and the selector are given; the check reads `NSMessage` too |
| Unverified platform behaviour (codebase M-15, readiness M17) | Marked UNVERIFIED, with fallbacks: App Intents (cut the intent from F1), notifications in tests (a `NotificationPoster` protocol), testteeth with nothing to mutate (skipped when `src/` and `eval/*.py` are unchanged) |
| `FeatureRows.swift` shape; core rows (readiness M12) | The type is given; rows are declared by each feature, not read from the API |
| `--resume` across commits (readiness M13) | The output path includes the commit |
| Gate fails for another scope (readiness M14) | The parent's rule: add a `<id>-fix-<n>` node with the failing scope |
| `genesis.yaml` not updated (readiness M15) | Added to B1 |
| Base plumbing for uncommitted features (zoom-out N2) | The `workflow` field moves to F6s, and each key to its feature's unit |
| Grant rule in four places (zoom-out N3) | One `once_only` function |
| `ContentEntry` may be v1's job (zoom-out N4) | Noted for the owner: B2 extends v1's list if U1 delivered one |
| Signatures in the architecture (zoom-out N5) | Moved to the plan |
| Reuse `cli_llm` helpers and the `url` crate; drop `heron undo --run` and `screen` (zoom-out N6) | Done |
| Run summaries kept in two stores (zoom-out N1) | The kv record is written at `session` and updated; memory is a cache |
| Nits: stale `HERON_CLI_TOPOLOGY`; Telegram refuses tainted asks; account `detail`; trigger names; Automations screen; audit session; the `outside` label; titles; gate concurrency; owner steps; the `ask_reasons = false` test; style nits | Each fixed in place |

## Not changed

*   None of the findings was contested.
*   One owner-visible change came out of this stage: F1's "Ask Heron" App Intent depends on an unverified build step. If that step fails, the intent moves to v1.1 and F1 keeps the panel and Services.
