# Stage 7: critic review of heron-v1-features.md (FeatureRange)

Status: resolved

*   **Target:** `docs/designs/heron-v1-features.md` at commit `ee485a7`, lines 1 to 631.
*   **Reviewers:** `egm-design` and `egm-structural` against `feature-spec-standard.md`, and `egm-style`. Each ran in a fresh context with only the target, the parent and `src/`.
*   **Result:** design 1 Blocker, 8 Major, 14 Nits; structural 0 Blockers, 2 Major, 10 Nits; style 0 Blockers, 7 Major, 17 Nits.
*   **Resolution:** all findings were accepted except structural Nit 12, which is answered in the text. The product effects of the fixes:
    *   F5 and F8 can no longer be granted beyond "once".
    *   F4 is renamed "Undo a run's file edits" and lists what it can't undo.
    *   F9 uses the system window picker.
    *   F1 has no default hotkey.
    *   F2 gains the core key `ask_reasons`.

## Design (`egm-design`)

| # | Finding | Decision | Change |
|---|---|---|---|
| 1 (Blocker) | `ft2` passes only if the model obeys the injection | Fix | `ft2` asks for a user-requested write after the page taints the session; `write_file` isn't approved, so an approval always happens |
| 2 | Standing and conversation grants would let `run_shortcut` and `draft_email` run unasked | Fix | `can_always` and `can_session` are false, `always_rule` returns nothing, and Settings offers only "ask" and "never" for these tools; this is in Q7 |
| 3 | "Window text is never stored" is false | Fix | The text now says chat context is stored and sent like any message, with a Risks row; only images are never stored |
| 4 | F4 overstates what it undoes | Fix | Renamed; it says only file-tool edits are restored; adds a `cannot_undo` list |
| 5 | `draft_email` doesn't validate addresses or name its parameters | Fix | Address regex, at most 10, only `subject` and `body`; a test and a Risks row |
| 6 | `ContentEntry` misses suggestions; no check that callers name an entry | Fix | `Suggestion` and `ScheduledPrompt` added; `mark_tainted` requires a `ContentEntry` |
| 7 | `ft6` unreliable; unclear what core keys control | Fix | Core keys control the tool list, workflow validation and execution; `ft6` turns the key on, waits on `wait_sql`, reads the id from the response, and checks the run record |
| 8 | Injection tasks can't detect a broken taint rule | Fix | Each injection task also targets `task_add` or `remember`; `ft5` now tests untrusted shortcut output |
| 9 | The Risks table leaves out the new security risks | Fix | Rows for grants, `mailto:`, stored text, the app-name attribute, taint sources, the lock screen, `notify_command` |
| Nits | Taint writes only after v1; the wording of `what`; `proactive` duplicates; frontmost rule; `quiet`; new mail every day; record timing; schedule estimate; `[features]` justification; F2 switch; Alternative C; F9 alternatives; "sandbox" wording; default hotkey | Fix | Each changed: "After v1"; a `what` column; `proactive` excluded; relayed notices always posted; `quiet` dropped; new mail left out of the brief; record written before `run_end`; estimate with base units; reason stated; `ask_reasons` key; Alternative C reworded; Alternative G added and F9 uses the picker; "Seatbelt profile"; no default hotkey (⌥⌘Space is Finder search) |

## Structural (`egm-structural`)

| # | Finding | Decision | Change |
|---|---|---|---|
| 1 | Tables don't map to the eight checks; no `PromptSource` row | Fix | Checks numbered 1 to 8; every table gains "Prompt blocks"; the mapping is stated |
| 2 | Bare `<placeholders>` are stripped as HTML | Fix | Every placeholder in user-facing text is in backticks |
| 3 | F10 has no table | Fix | Stated: F10 isn't planned for v1 |
| 4 | Diagram misalignment and backticks | Fix | Redrawn |
| 5 | Behind / At par / Ahead not headings | Fix | Now H4 headings |
| 6 | Work unit ids used before they are defined | Fix | A "Work unit ids" row in Definitions |
| 7 | Status carries prose; extra "Answers" field | Fix | Status is "Proposed"; the Q8 note is a sentence below the metadata |
| 8 | "Future Improvements (v1.1 candidates)" heading | Fix | Heading restored; the note moved into the body |
| 9 | Stray space in JSON | Fix | Removed |
| 10 | CUJs cover F1 to F5 only | Fix | One line says so |
| 11 | Definitions after Non-Goals; Baselines under Overview | Fix | Definitions moved before Goals; Baselines folded into Definitions ("Today", "After v1") |
| 12 | Links to heron-v1.md don't resolve yet | Contest | Already disclosed in Definitions; the PR body repeats it. The merge order is the owner's call |

## Style (`egm-style`)

| # | Finding | Decision | Change |
|---|---|---|---|
| 1 | "better than any of them" | Fix | "no surveyed product documents" |
| 2 | Unsourced 341 / 2,857 | Fix | Attributed to Koi Security through the research note, marked as a search summary |
| 3 | "reports clearly" can't be tested | Fix | It names the permission and the System Settings pane, in the feature's row |
| 4 | "eight" against nine rows | Fix | See structural 1 |
| 5 | "strongest promise", unsupported claim about users | Fix | Reworded to facts |
| 6 | Debug-build claim stated as fact | Fix | What is the same and what differs, marked as an assumption, checked by the release G7 run |
| 7 | "what friends would accept" | Fix | Replaced by "records third parties without consent" |
| Nits 1–17 | Hedges, fragments, vague quantities, sentence order, unclear rebase | Fix | Each reworded |
