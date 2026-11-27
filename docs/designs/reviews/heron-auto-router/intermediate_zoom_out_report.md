# Stage 8, Intermediate Zoom Out: heron-auto-router.md

Reviewer: `egm-zoom-out` (Goldfish). Target at commit e9d72cd. Verdict: ready once two Majors are fixed (0 Blocker, 2 Major, 6 Nit). The eval-first structure and the gate survived the later stages; the live run, held-out set, genesis audit, pin and Definitions were judged proportionate. All findings were fixed.

| id | finding | fix |
|---|---|---|
| M1 | The Opus allowance added a kv ledger that the per-model `usage` table already provides (`src/llm.rs:476-479`, `src/store.rs:1670-1675`) | `Budget::deep_left` sums today's deep rows; no ledger, no charging hook |
| M2 | The tier-refusal retry repeated `HttpLlm`'s fallback chain (`src/llm.rs:376-379`) and put model fallback in the agent loop | Fallback stays in the `Llm` layer: `CliLlm` gets the same requested-then-standard chain; the loop only names the model that answered; no `tier_down` marks or `down` input |
| N1 | The Overview said six units; the table has eight | Fixed |
| N2 | Struct fields drifted across the diagram, §3 and item 2 | The full signatures live in item 2 only |
| N3 | `Report.deep_today`/`deep_limit` and `--deep-model` had no consumer | Dropped |
| N4 | `CallOutcome` had five variants, one distinction read | `tool_failed: bool` |
| N5 | The delegate child got two values | One fixed tier through rule 1; rule 3a removed |
| N6 | RQ7 before RQ6; pre-step named RQ1 to RQ6 | Renumbered; pre-step says RQ1 to RQ7 |
