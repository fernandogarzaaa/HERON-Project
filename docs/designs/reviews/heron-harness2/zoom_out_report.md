# Stage 5: Initial Zoom Out review, heron-harness2.md

*   **Reviewer:** `egm-zoom-out` (Goldfish: given only the doc and the repository paths it needed)
*   **Target:** the first draft, commit `705fa05`
*   **Status:** resolved in the second draft

The reviewer found 16 issues: 1 Blocker, 9 Major and 6 Nits. Each row gives what the second draft did about it.

| # | Severity | Finding | Resolution |
|---|---|---|---|
| 1 | Blocker | The fence allowed `HeronCore/**` and `HeronUI/**`. Allowed stores and the client can call `setMode`, `removeNever`, `acceptRiskNotice` and `approve` (`CoreClient.swift:187-312`, `SettingsAndFirstRunStores.swift:134-251`) | **Fixed.** `HeronCore/**`, `Stores/**` and `Views/**` are now fenced. Only `Tokens/**` and `Avatar/**` are auto-eligible, and they reference no store or client (checked by grep, and enforced by new rule R7). The HQ1 count was redone: 0 of 30 |
| 2 | Major | Undo and Return to main can't pass the decision table | **Fixed.** Both are now owner clicks outside the table. Undo unzips the one kept previous app, and is disabled if `user_version` changed. Return to main runs a build plus the health check only (§9) |
| 3 | Major | Phase the work: build the PR pipeline first, and add auto-install once its share is measured | **Fixed.** New HQ22. Phase 1 ends every run as a PR. Phase 2 (swapper, signing, rollback) has measured entry criteria. OD1 is kept; only the order changes |
| 4 | Major | The gate isn't scoped to what changed, the release builds run twice, and the G7 baseline in `$TMPDIR` isn't durable | **Fixed.** The gate steps are chosen from the changed paths (§4). G7 smoke now runs on base and fix, with the base result cached by commit in `~/.heron/selfheal/g7-base/`. `build-app.sh` gains `--no-dmg --host-arch`, and the separate release builds are gone |
| 5 | Major | `src/eval.rs` holds the retrieval thresholds, and R1 doesn't cover inline tests | **Fixed.** `eval.rs` is now fenced. R1 covers the `#[cfg(test)]` modules of the allowed core files. The name-list comparison was dropped |
| 6 | Major | `--identity` repeats `build-app.sh`'s default mode, and the identity should match the release DMG's | **Fixed.** HQ5 now uses the default mode with `CODESIGN_IDENTITY` set to the release Developer ID. The default mode drops `--deep` and adds `--options runtime`. P2 now tests a switch between a release build and a self-fix build |
| 7 | Major | The second pipeline for untrusted triggers is built for rare triggers | **Fixed.** Removed. Untrusted inputs become inbox notes only, and the owner restates them in their own words to get a trusted run (Alternative F) |
| 8 | Major | The scout reinvents the fetcher and the inbox | **Fixed.** It fetches through `tools::web::get_limited` and posts with `Store::add_inbox`. The candidates table and the seeded candidates were dropped |
| 9 | Major | A second panic hook, and a `message_template` that can't be built | **Fixed.** The existing hook in `main.rs:714-722` is extended. Records keep `kind` and `location` only |
| 10 | Major | The 9-step effect journal is heavier than recovery needs | **Fixed.** Replaced by a `swap_intent` marker and publishing steps that check the world first. Recorded as Alternative G |
| 11 | Nit | The zip report format needs a dependency | **Fixed.** `.heronreport` is now plain JSON |
| 12 | Nit | Stacked PRs aren't needed | **Fixed.** Every PR targets `main`. New rule R8 makes a fix "needs owner" when it overlaps an open self-heal PR |
| 13 | Nit | `src/ui/**` and docs-only diffs gain nothing from a reinstall | **Fixed.** `src/ui/**` is fenced, and a docs-only diff gets a PR with no install |
| 14 | Nit | Altitude: §6 and §7 hold step-level detail | **Fixed.** Phase 2 (§9) now lists invariants. The step list moved to the Phase 2 delta |
| 15 | Nit | The `ContentEntry` variants were claimed but not planned | **Fixed.** The four variants are now in item 16, and `every_content_entry_is_trusted_or_taints` is in H2's check |
| 16 | Nit | The pre-swap data backup duplicates the migration snapshot | **Fixed.** It is dropped, and §9 relies on the existing `heron-before-upgrade-from-vN` snapshot |

The second draft also folds in two owner additions that the session that started this one passed on during Stage 5:

*   OD2: connectors (§7);
*   OD3: observation and keystrokes through sealbox (§8).

Stages 6 to 9 review that draft.
