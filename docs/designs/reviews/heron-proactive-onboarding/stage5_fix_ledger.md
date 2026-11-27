# Stage 5 fix ledger

All Stage 5 findings were accepted (none contested). The doc was rewritten in one pass; the relayed heartbeat research was folded in at the same time.

| ID | Resolution |
|---|---|
| B1 | New `approve` action does the work in the core; `accept` is status-only (legacy rows still return `prompt` for Slint). `GET /api/suggestions` lists catalog cards only with `?catalog=1`; the schema step expires open legacy rows. §6, §11 |
| M1 | Journal left untouched; per-account budget dropped; dedup by card key, caps in `heron.db`. OQ7, Alternative A |
| M2 | `POST /api/onboarding` removed. `POST /api/sources/about` takes lists; one memory per field; replace by prefix for every caller; new `GET /api/sources/about` prefills from memories. Settings go through their own routes. Step 5 "Help me with" removed. §10, Alternative F |
| M3 | Intensity is a preset over `proactive_interval_mins`, `proactive_quiet_secs` and two new caps; `custom` when edited. Event passes deferred (Alternative J). §5 |
| M4 | `observation` source and proposer-made `create_automation` removed; Harness2 rows carry the kind and are approved through their session. §4, §6 |
| M5 | `follow_up` removed (inbox-only mail). §4, Future Improvements |
| M6 | OQ5 default changed: every card run tainted, catalog cards suggest-only; the outside/local split, `tasks.outside` and `is_outside` dependency removed. The split is Alternative E |
| N1 | Kept rules and per-kind bar; dropped intensity bar shift, auto-pause and expired-as-decline |
| N2 | Metrics in `GET /api/awareness` and the CLI, a small Settings panel; `proactive_notify` dropped |
| N3 | Persona presets are Swift constants |
| N4 | 4000 |
| N5 | Documents are `knowledge` rows |
| N6 | One memory per field (avoids sibling dedup); test names no longer assume `Unchanged` |
| N7 | `run_triggers`, `reindex`, `observe`, `reflect_if_due` stay behind Awareness (§1 table) |
| N8 | genesis must return `TRUSTED` |
| N9 | `/api/awareness` extended; no `/api/proactive` |
| N10 | Banner dropped; OQ9 alternative |
| N11 | `ChoiceChips` styled from `HTag` and `HButtonStyle` |

Relayed research added: pass record and one-time unreachable note (§2, CUJ P-8); "Not now" is one-time (§7); OQ13 (active hours), OQ14 (checklist), OQ15 (nightly reflection); Alternatives H-J; Additional Resources citations marked UNVERIFIED where relayed.
