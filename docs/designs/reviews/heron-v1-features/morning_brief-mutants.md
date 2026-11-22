# F6 morning brief: surviving mutants

F6's gate (`scripts/verify-feature.sh morning_brief`, testteeth against `features_base_commit` 01a977d) scored 93.8%, with one survivor.

- `src/tools/personal.rs` `local_midnight`, `t += chrono::Duration::hours(1)` -> `t -= …`: the line runs only when local midnight doesn't exist on that date, which happens only in a time zone whose clock change skips midnight (for example some years in Santiago or São Paulo). `chrono::Local` reads the process time zone, so a test would have to change `TZ` for the whole test binary while other tests run in parallel, which makes them race. On every other day the first `earliest()` succeeds and the loop body never runs. The mutant is left, and the line's comment states the intent.
