# Stage 6: comprehension test of heron-v1-features.md

Status: resolved

*   **Target:** `docs/designs/heron-v1-features.md` at commit `b902b1f`. FeatureRange is lines 1 to 632; DetailedRange is line 633 to the end.
*   **Reviewer:** `egm-comprehension`, a fresh context, given the target, the parent `heron-v1.md` and `src/` on `main`.
*   **Verdict:** not re-implementable as written. 8 Blockers, 15 Majors and 6 Nits. The worst gaps were in the eval harness, F4, F5 and F6.
*   **Resolution:** every finding was accepted. Fixing them added one behaviour change: F6 now ships off (QF3), because turning it on creates a schedule.

| # | Finding (short) | Fix in the doc |
|---|---|---|
| 1 | `POST /api/schedules` can't create a step automation | The route gains an optional `workflow` field, validated by the workflow checker. F6 gives the exact JSON |
| 2 | F6's condition and text can't be built from the tool output | `today_counts` returns two lines: the total, then the sentence. The template uses `lines \| first` and `lines \| last` |
| 3 | `fs::undo` handles only the newest checkpoint | New `fs::undo_checkpoint(id, force)` shares the check. A later run's checkpoint on the same path is a conflict. `force` restores that one file and continues |
| 4 | `heron undo --run` has no source for the run | At `run_end` the core writes kv `run:<run_id>` with the checkpoint and audit id ranges. The CLI reads the database directly (Q3) |
| 5 | Nothing writes the shortcuts allowlist | `POST /api/shortcuts/allowed {"names": [...]}` |
| 6 | The content-entry list is undefined and miscounted | B2 adds `ContentEntry`, listing every entry including MCP, Telegram forwards and trigger events, with the test `every_content_entry_is_trusted_or_taints` |
| 7 | The taint source value is under-specified | The fields are defined for each kind. One `mark_tainted` function replaces the writes in `agent.rs`, `app.rs`, `telegram.rs` and `mcp.rs` |
| 8 | No wording table for `why()`; it conflicts with the old warning | A wording table, and the function's inputs. A parity test covers untainted inputs. The Slint app isn't changed |
| 9 | The notification payload is undefined; `notice` already exists | A new bus type `notification {title, body, source}`. The core counts subscribed `notify=1` clients and falls back when there are none |
| 10 | F3's run states, title and notification rules are incomplete | The state machine, where the title comes from, and which origins get "Done" |
| 11 | Defaults contradict each other; the route shapes are missing | Every core key defaults off, and the app turns on `today_counts` with the brief. Tools register live. Request and response shapes are given |
| 12 | The `run_shortcut` contract is incomplete | Input, output file, exit, timeout, run folder and the `GET /api/shortcuts` shape |
| 13 | `also_changed` and the undo audit row have no shape | Both shapes, the tool list, and the `404` and `409` cases |
| 14 | F5, F6 and F8 change the decision-table test but say "Q7: no" | They now say Q7: yes (`permissions.rs` tests) |
| 15 | "Risk classes" is undefined | Now "which `Effect` values" |
| 16 | Text from the intent isn't classified | Intent text arrives as chat context of kind `intent` (Outside), shown as a chip. The owner's own words go in the field |
| 17 | `--tasks` already means task ids | The new flag is `--task-file` |
| 18 | `check_tasks.py` rejects feature files | `check_tasks.py --features` has its own rules, with new keys `features`, `injection`, `config`, `drop_stream` and `after`, and `ft` ids |
| 19 | The feature tasks can't be expressed | New task keys `after` (HTTP calls made after the turn) and `drop_stream`, and new rules `no_executed`, `event_matches` and `http_json`. `prompt` may be null |
| 20 | Fake binaries are debug-only, but the gate runs a release build | `--features` mode requires `build = "debug"`. A task's `config` sets `shortcuts_allowed` |
| 21 | The Settings, Features screen exists only on F1's branch | New base unit B4 (`macos/**`) |
| 22 | B3 edits `docs/` | New base unit B5 (`docs/**`) |
| 23 | "Feature key" means two things | New term "slug" |
| 24 | `--features` requirements unstated | Stated |
| Nits | 400 body; scope of speech; panel sessions; source app name; checkpoint times in whole seconds; Slint; gates need the owner's Mac; base units untested | All addressed: id ranges replace time spans; base gate unit B6 |
