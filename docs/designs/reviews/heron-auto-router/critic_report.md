# Stage 7, Critic Review: heron-auto-router.md (lines 1-443)

Reviewers: `egm-design` (1 Blocker, 5 Major, 7 Nit), `egm-structural` (4 Major, 8 Nit), `egm-style` (3 Major, Nits). Target at commit 96eee52. Resolution: every finding fixed except where noted.

| source | finding | decision |
|---|---|---|
| design 1 (Blocker) | The gate's cost compared unlike things: single-model runs priced a Sonnet learning call into every Haiku turn, and pinned runs hid the router's escalation cost | Fixed. Single-model setups set `--fast-model` to the same model; the tilt is stated both ways; a live run of the built router (unit L) must pass the same gate before RQ1's default-on |
| design 2 | Word lists tuned and gated on the same 40 cases; noise | Fixed. Lists frozen before labelling; later changes are scored on 20 held-out cases; the tolerance is stated as one case's worth |
| design 3 | A failing shell command isn't `ToolFailed`; escalation never measured | Fixed. A non-zero `[exit code N]` counts; the live run measures escalation |
| design 4 | A refused Opus or Haiku tier kills the turn; no Risks section | Fixed. Retry on standard, mark the tier down for 30 minutes, notice; a Risks & Mitigations section |
| design 5 | With the default caps of 0, nothing limits Opus | Fixed. RQ7: a daily Opus allowance, `deep_tokens_per_day` = 1,000,000 |
| design 6 | A deep pin outranks Minimal and Spent, against G5 and the notices | Fixed. G5 states the override as an explicit owner choice, like `routing = "off"`; the notice names the pin; a delegate child's inherited tier (rule 3a) yields to rule 3 |
| design nits | Budget read once per turn; absent-tier moves; delegate error outcome; `plan`/`prove` false positives; `Report.main_model`; price mapping; labelling cost | Fixed: budget re-read per step; no moves into absent tiers; outcomes named; bare `plan` and `prove` dropped, near-miss cases added; `main_model` shows the standard tier; mapping defined; about $80 at list prices in RQ4 |
| structural 1 | No Risks & Mitigations section | Fixed (with design 4) |
| structural 2 | Unit IDs used in the spec but defined only in the plan | Fixed. The Overview lists the six units |
| structural 3 | Resources and Future Improvements nested under NFR | Fixed. Promoted to H2, with The eight checks as its own H2 |
| structural 4 | "Owner questions" instead of the template's "Open Questions" | Fixed. Renamed, with a note that technical unknowns are the Prototype checks |
| structural nits | Glossary in the Overview; TCC; G9 collision; provenance; PR links; duplicated prices; CUJ 3's When | Fixed: a Definitions H2; provenance at the end; PRs linked as PRs; prices cite one source; CUJ 3 rewritten. G9: always written as heron-v1's G9 |
| style 1 | G1 and the gate defined "matches" differently | Fixed. G1 points to §5.6 only |
| style 2 | "Two config-only setups" against five baselines | Fixed |
| style 3 | "The one behaviour change in config" was incomplete | Fixed through G5, RQ7 and CUJ 7, which list the changes for existing configs |
| style nits | Fable price, unhedged claims, 0-based step example, models vs tiers, performance figure | Fixed |
