# Stage 5, Initial Zoom Out: heron-auto-router.md

Reviewer: `egm-zoom-out` (Goldfish, fresh context). Target read at the first draft (commit 63c62f1). Verdict: not ready (1 Blocker, 7 Major, 4 Nit).

| id | severity | finding | decision |
|---|---|---|---|
| B1 | Blocker | The "does the router earn its complexity" check came after the build, and left out today's two-tier config (`cli_model = opus`, `fast_model = haiku`) as a baseline. Line 41's claim that chores would run on Opus was false (`req.model` wins, `src/cli_llm.rs:2237-2244`) | Fixed. The eval (E0) and the rules (R1) come first; labels need no router code (harness writes `cli_model`/`fast_model`); unit G gates R2-R4; both two-tier configs are baselines |
| M1 | Major | Spent doesn't stop a turn today (`src/usage.rs:171`, `:213`) | Fixed. Spent routes like Minimal; `Decision.tier` is a plain `Tier` |
| M2 | Major | Rule order put the budget before "off", against `src/usage.rs:209-211` and its test at `:337-339` | Fixed. "off" is rule 2, right after the pin |
| M3 | Major | Schedules, takeover, workflow steps and delegate children carry outside or model-written text into `decide` | Fixed. Only typed turns (`run_turn_typed`) in untainted sessions are matched against deep words; a delegate child runs on its parent's tier |
| M4 | Major | Dollar estimates in the core and Usage tab are scope creep | Fixed. Prices live only in `eval/router/prices.json`; no core or Mac change |
| M5 | Major | The per-session pin is unrequested | Contested in part. The owner's brief asks for a pin. Kept per session; `default_pin` dropped (a global pin already exists as `routing = "off"` plus `cli_model`) |
| M6 | Major | Rule 6 (follow-ups), `last_tier` and `context_items` can't be measured | Fixed. All three removed |
| M7 | Major | `[llm.tiers]` duplicates `fast_model`/`model` and silently overrides `fast_model` | Fixed. Tiers come from `fast_model`, `cli_model`/`model` and one new key, `deep_model`; no env vars; effort dropped from this build (Non-Goal) |
| N1 | Nit | `Config::fast_model()` would be dead code | Fixed. It returns the quick tier |
| N2 | Nit | The genesis-audited verifier didn't feed the metric | Fixed. `score.py` scores through `router_verifier.py` |
| N3 | Nit | Struct and signature names drifted | Fixed. One `TurnSignals`, one pure `escalate` signature |
| N4 | Nit | Wrong refs (`cli_llm.rs:437`, `usage.rs:231`), missing `CoreEvent.swift:27` and `AvatarStateMachine.swift:81`, no harness change for pinning | Fixed |
