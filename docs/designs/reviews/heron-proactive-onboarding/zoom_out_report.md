# Stage 5: Initial Zoom Out Review

**Target:** `docs/designs/heron-proactive-onboarding.md` (full file, draft `4b32b9f`)
**Reviewer:** `egm-zoom-out` (fresh context; given only the doc, the standard, Harness2, the PR #72/#83/#84 snapshots and the cited code)
**Status:** resolved (see `stage5_fix_ledger.md`)

## Findings

| ID | Severity | Finding | Lines (draft) |
|---|---|---|---|
| B1 | 🔴 Blocker | The new `accept` behaviour breaks the Slint desktop app (`src/bin/desktop/views_bridge.rs:352-369`, `views_slint.rs:1282-1290`, `src/ui/views.rs:1010-1015`): it either keeps laundering or makes "Got it" and "open prepared session" start work. The route's contract (returning runnable text) is the root cause. Give core execution its own action; keep `accept` status-only | 35, 81, 146, 374, 395, 580-581, 687 |
| M1 | 🟡 Major | Wiring `ProactivityJournal` adds a second dedup system, a third cap and a second database; consent `valid_until`, `Sensitivity` and per-card charging are undefined. Leave it untouched | 131, 292-299, 331, 365, 604, 826-840 |
| M2 | 🟡 Major | `POST /api/onboarding` duplicates the settings, persona and awareness routes, keeps answers in kv and memories (two sources of truth), sits in the wrong module, and step 5 "Help me with" repeats step 3's goals. Extend `About`/`save_about` with lists and replace by prefix for everyone | 72, 517, 531-540, 588-589, 656, 777-783, 966-971 |
| M3 | 🟡 Major | Intensity overlaps `proactive_interval_mins` and `proactive_quiet_secs` with no precedence; event passes add little and the "known sender" rule is undesigned. Make intensity a preset over the existing keys; defer event passes | 261, 359-367, 546-551 |
| M4 | 🟡 Major | `create_automation` from observation memories duplicates Harness2 §8.7's producer. Drop it from the proposer; Harness2's rows carry the kind | 285, 349, 426, 606 |
| M5 | 🟡 Major | `follow_up` can never fire: every mail source indexes the inbox only | 343 |
| M6 | 🟡 Major | The outside/local split exists only to keep `prepare`/`takeover`; local-evidence run cards with model `ask` text start untainted. Offer the owner: every card run tainted, catalog cards suggest-only | 76, 126-127, 265, 291, 355, 387, 431, 560, 565, 837 |
| N1 | 🟢 Nit | Overlapping learning controls: keep rules and the per-kind bar only | 70, 147-148, 366, 413-415 |
| N2 | 🟢 Nit | Metrics in three places; `proactive_notify = "all"` not needed | 430-449, 551, 584 |
| N3 | 🟢 Nit | Persona presets served by the core break the Swift-constants rule | 526, 859 |
| N4 | 🟢 Nit | `persona::MAX_CHARS` is 4000, not 2000 | 526 |
| N5 | 🟢 Nit | Documents are `knowledge` rows, not a table | 282 |
| N6 | 🟢 Nit | Same-words re-save returns `Updated`, not `Unchanged`; sibling chips sharing a prefix may dedupe into each other | 499-502, 535 |
| N7 | 🟢 Nit | Gate change silent on `run_triggers` and `memory.reindex` | 259 |
| N8 | 🟢 Nit | genesis verdict word: Harness2 expects `TRUSTED` | 1012 |
| N9 | 🟢 Nit | Extend `/api/awareness` instead of a parallel `/api/proactive` | 584-591 |
| N10 | 🟢 Nit | The upgrader banner overlaps the Settings tab and Run setup again | 144, 542 |
| N11 | 🟢 Nit | Build `ChoiceChips` on `HTag` and `HButtonStyle` | 487 |

The reviewer's prerequisite checks passed: the context is clear, the design targets the real causes, and extending `proactive_tick` (rejecting Alternative A) is sound.
