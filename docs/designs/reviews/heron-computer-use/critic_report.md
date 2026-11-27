# Stage 7, Critic Review: heron-computer-use.md (lines 1-537)

Reviewers: `egm-design` (2 Blocker, 11 Major, 6 Nit), `egm-structural` (3 Major, 8 Nit), `egm-style` (5 Major, 16 Nit). Target at commit 96eee52. Resolution: every finding fixed except where noted.

| source | finding | decision |
|---|---|---|
| design 1 (Blocker) | The core and its children (shell, CLI, MCP servers, which run unsandboxed, `src/mcp.rs:41`) may inherit the app's Accessibility grant | Fixed. PT-C7 is a ship condition; the core is launched with responsibility disclaimed if needed; no ship if no launch mode keeps the grant inside the app. Recorded in R-2 |
| design 2 (Blocker) | `/api/cancel` aborts the run task (`src/server.rs:645-653`), so the permit outlived the turn | Fixed. A `PermitGuard` drop guard ends the permit and the action entry on every exit; the app drops the permit on `run_end` |
| design 3 | The core couldn't tell a step card or pause from a dead app; two start shapes | Fixed. `POST /api/computer/hold`; §3 uses the §2.1 shape |
| design 4 | **Run again** couldn't reach the app and would run the goal with every tool | Fixed. The resumer adds a drive card and runs no turn |
| design 5 | Concurrent permits; remote Stop route | Fixed. One live permit per desktop (`busy`); Telegram `/stop` and `POST /api/computer/end` end it |
| design 6 | Newlines in `type`, bare Delete, global chords, icon-only buttons, missing verbs and roles | Fixed. Line breaks and bare Delete on a selection are irreversible; a chord allow list; unlabelled controls are carded; the word and role lists extended |
| design 7 | Links, the Services menu, open/save panels, missing shells and remote-control apps | Fixed. Links are carded, Services refused, panel actions carded; deny list extended, including the developer-tools category; NFR claims softened |
| design 8 | Typing into shared documents is sharing; messaging apps other than Mail and Messages | Fixed. Every messaging app hard-denied (CQ7); shared documents are CQ12, recorded in R-2 |
| design 9 | No token, cost or latency budget | Fixed. Only the latest two observations stay in context; estimates per step, for CUJ 2 and for a 60-step permit; PT-C8; default time raised to 20 minutes |
| design 10 | Blocking Accessibility calls and the 200 ms stop | Fixed. Driver work on a serial background queue, a 1 s messaging timeout, a stop check between typed chunks |
| design 11 | No fallback if a prototype check fails | Fixed. Each of PT-C1, C3, C4 and C7 has a fallback or a no-ship rule |
| design 12 | Posted input could land elsewhere | Fixed. Hit test before posted clicks, `postToPid`, focus-then-type |
| design 13 | The loop bound can't change mid-turn | Fixed. The bound becomes a variable: `max(bound, k + max_steps + 10)` |
| design nits | "Heron paused" two ways; G4 vs pause; start in Read-only mode; which answers count as steps; Screen Recording only for crops; R-2 contents | Fixed. Pausing Heron ends the permit; G4 says refuse and pause; start not offered in Read-only mode; every answered action counts except `paused`; **Screen Recording deferred to Phase 2** (step cards drawn from Accessibility data); R-2 lists each accepted risk |
| structural 1, style M1 | Two start-request shapes | Fixed |
| structural 2 | Diagram border | Fixed |
| structural 7 | `<goal>` and `<app>` vanish in rendered Markdown | Fixed (code font) |
| structural nits | Open Questions name, Definitions H2, Risks rows, refs, provenance, URL link | Fixed. The Risks section stays an H3 under NFR, as in heron-harness-safety.md; it now has rows for CQ2, PT-C7 and the cancel path |
| style M2 | The 200-step bound read two ways | Fixed (see design 13) |
| style M3, M4 | Unquantified "slower by hand", "far more tokens" | Fixed: "20 approvals for a 20-step task"; token figures |
| style M5 | "Every number" | Fixed. Scroll range and new numbers added; the sentence softened |
| style nits | Pronouns, figurative wording, tense, CUJ 6's goal, names, word-list case | Fixed where the text still exists; CUJ 6 kept as a continuation of CUJ 3 |
