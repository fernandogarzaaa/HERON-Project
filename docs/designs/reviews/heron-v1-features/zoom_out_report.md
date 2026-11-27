# Stage 5: initial zoom-out review of heron-v1-features.md

Status: resolved

*   **Target:** `docs/designs/heron-v1-features.md`, full file, at commit `c4e5833`.
*   **Reviewer:** `egm-zoom-out`, a fresh context, given the target, the parent design `heron-v1.md` (approved copy at `cba31ab`), `src/` on `main`, `docs/COMPARISON.md` and the research note.
*   **Result:** 1 Blocker, 9 Major, 7 Nits.
*   **Resolution:** the author accepted every Blocker and Major and rewrote the doc. N3 and N7 (on Messages) are answered in the text rather than changed. Accepting them changed the product proposal: F4 is proposed for v1.1, and F5, F6 and F8 are smaller. Those changes are in QF1 for the owner.

| ID | Finding (short) | Decision | What changed |
|---|---|---|---|
| B1 | F7's App Intent can run with nobody present, which gets around Q6 and inflates G9 | Fix | The intent opens the quick-entry panel with its text filled in, and the owner presses Return. `source: "intent"`, its taint kind and its test are gone |
| M1 | One branch and one PR for ten features is the only reason for `DEFERRED` and a freeze commit | Fix | Each feature is its own small skein graph and its own PR. The freeze is "gate passed by the date". `DEFERRED` is gone |
| M2 | App-only toggles and TCC knowledge in the Rust core | Fix | `[features]` in the core holds only features that add tools. App-only toggles live in `UserDefaults`. `Feature::needs()` and the off-macOS rule are gone |
| M3 | F4 duplicates F7, has the QF4 unknown, and would duplicate events | Fix | F4 is proposed for v1.1 (QF1). If kept, it is a calendar source on the existing source routes, with duplicate handling stated |
| M4 | F8 rebuilds a scheduler the core has | Fix | F8 is a built-in automation template with no model step, plus one `today_counts` tool |
| M5 | F5 adds a second undo engine and journal | Fix | The v1 cut undoes the run's file changes through the existing undo and lists its other changes with links. Full state undo is v1.1. No new table and no `store.rs` schema change |
| M6 | F3 rebuilds finished runs from a replay ring | Fix | A bounded in-memory list of run summaries, filled on `run_end`. The doc separates v1's `run_id` runs from scheduled-job history |
| M7 | Two notifications per job (core `osascript` and the app) | Fix | One owner: while the app is subscribed, the core publishes `notice` on the bus and doesn't call `notify()` |
| M8 | `features.shortcuts` is both a boolean and a table | Fix | `shortcuts = true` and `shortcuts_allowed = [...]` |
| M9 | F2 rebuilds the existing approval explanation | Fix | The wording logic of `approval_warning` moves to the core. The taint source goes into the value of the existing `tainted:<session>` key. `decide()` and the decision table don't change |
| N1 | Two ways to start a turn tainted | Fix | Fixed by B1 |
| N2 | F6 overlaps macOS dictation | Fix | F6 uses system dictation and adds spoken replies. No Microphone or Speech permission for Heron |
| N3 | F9 could use the `open` crate | Contest, explained | `/usr/bin/open` stays, because the `open` crate passes Heron's full environment and can't be faked in tests. The doc says so |
| N4 | Timing tests would be flaky | Fix | Times are reported, not gated |
| N5 | "F1 to F8 committed" doesn't fit the window | Fix | The commitment is F1 to F5. The rest are built in rank order until the freeze |
| N6 | The undo claim goes past the evidence | Fix | Softened to "none advertises it (unverified)" |
| N7 | Mixed route; signatures at the architecture level; Shortcuts and Messages | Fix | The shortcut list has its own route; the signatures moved to the implementation plan; the Messages exception is stated |
