# Stage 6, Comprehension Test: heron-computer-use.md

Reviewer: `egm-comprehension` (Goldfish). Target at commit ccd395f. Verdict: 3 Blocker, 13 Major, 10 Nit. All findings were accepted and fixed.

| id | finding | fix |
|---|---|---|
| B1 (Blocker) | Tools and prompt are built once per turn (`src/agent.rs:486-487`), so a mid-turn permit never changes what the model sees | The step loop rebuilds both when the permit state changes; `execute_call`'s refusal holds in between |
| B2 (Blocker) | Waits conflict with `tool_timeout_secs` (120 s); no sheet wait; no timeout behaviour | Computer tools get the shell's exemption and their own waits (sheet 10 min, action 30 s, step card until the permit ends); a timeout ends the permit with `app_timeout` |
| B3 (Blocker) | Wire contract incomplete | §2.1 fixes the request, result, ready, end, `permit_ended` and error-code JSON, plus the `keys`, `direction`, `amount` and `app` formats |
| M1, M13, N1, N2 | Undefined vocabulary | A Terms block links heron-v1.md, heron-v1-features.md, PR #84 and Harness2, and defines the build terms |
| M2 | Permit end conditions scattered; step counting split | One table of every way a permit ends; the app counts steps and returns `steps_used` |
| M3 | Pause undefined | The app holds the request; the turn waits; Resume runs it; the clock keeps running |
| M4 | Permit steps vs the 40-step turn limit | A permit step is an action; a driving turn's limit is `max_steps` + 10, within 200 |
| M5 | How tool entries link to the action entry | Run id `drive:<EntryId>` while a permit is live |
| M6 | Suggestions have no session until prepared (`src/store.rs:1600-1604`) | Accepting a `drive` card creates a tainted session and returns it |
| M7 | How takeover is kept from driving | An `offers_computer` flag, set only for Mac-app chat turns and drive-card sessions |
| M8 | Which copy of the allow list is authoritative | Allowed apps and extra words live only in the app's own settings |
| M9 | Four tools during a permit, not five | Fixed |
| M10 | Secure-field label matching | Whole word, ignoring case |
| M11 | "Action log" undefined | The chat's existing tool-call rows |
| M12 | Step card decline vs its buttons | **Allow once**, **Skip** (`declined`, permit continues), **Stop driving** |
| N3 | Secure Input vs the class | The class is renamed `secure_field` |
| N4-N8 | Cap order, tolerance point, settle rule, first-action baseline, un-permitted front app | Draft parameters and §4.2 define each |
| N9 | Undefined and duplicate types | `PermitSlot` and `DriveTurn` defined |
| N10 | No URL for Anthropic's guidance | Added |
