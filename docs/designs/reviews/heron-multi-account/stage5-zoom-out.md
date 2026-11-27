# Stage 5: initial zoom-out review (resolved)

Status: resolved

**Target:** `docs/designs/heron-multi-account.md` (full file, draft `0099c1c`)
**Reviewer:** `egm-zoom-out` (Goldfish, given only the doc, the provider-facts file and the frozen PR #72 tree at `614d681`)
**Result:** 0 Blockers, 5 Majors, 8 Nits. 12 fixed, 1 contested.

| # | Severity | Finding | Decision | What changed |
|---|---|---|---|---|
| 1 | Major | The read-side source lists came from the Keychain registry, although `search`, `upcoming` and `connected_summary` take only the store. | FIX | §3 Aggregates now use store family queries. Only `learn` uses the registry. The search-result account label moved to Future Improvements. Alternative F added. |
| 2 | Major | `Legacy` was a permanent second runtime path for cases that barely exist. | FIX | `Legacy` is removed. §8 files every old connection under one naming rule. A fallback-file token is listed as "Sign in again". A connection with no subject gets the fixed ID `account_id(p, "")`. The cost (no sync until the next start after a failed migration) is stated. Alternative E added. |
| 3 | Major | The cap was enforced in three places, and one of them used the display label. | FIX | The cap is checked only at admission, by `sub`. The start-time check is removed. |
| 4 | Major | The Work/Personal kind tag was speculative. | FIX | Q-3 is removed and the tag moved to Future Improvements. |
| 5 | Major | Microsoft tenant support was built with no client to test it against. | FIX | §12 keeps it as plan text. The tenant variable and build passthrough moved to the change that registers a client. U1 and U4 no longer build them. |
| 6 | Nit | There were too many Microsoft message rows. | FIX | Three Microsoft rows remain: admin block, declined, and generic with its code. |
| 7 | Nit | `setup_glue.rs` doesn't need to change. | FIX | Confirmed at `setup_glue.rs:991-998` and `:1105-1107`. Item 7 now says "No change", and the desktop test moved to `verify`. |
| 8 | Nit | `has_source_rows` duplicated existing functions. | FIX | Removed. The backup now runs whenever there is an old token to migrate. |
| 9 | Nit | `disconnect_source` didn't need two edits. | FIX | `disconnect_source` keeps its exact `SOURCES.contains`. Only `learn_one` accepts per-account names. |
| 10 | Nit | The hash-collision branch and its test were ceremony. | FIX | The dedicated row and test are dropped. A subject mismatch now fails with the generic error. |
| 11 | Nit | A second map was kept just for sign-in attempts. | FIX | There is now one sign-in record map keyed by `attempt`, holding `state` and `Pending`. |
| 12 | Nit | Code signatures sat in architecture sections. | FIX | §3–§5 now describe behaviour. The signatures moved to the Detailed Implementation Plan. |
| 13 | Nit | Fold the account metadata into one JSON record. | CONTEST (reversed in stage 7) | Kept one key per field at first, to match the existing pattern. Stage 7's design critic then showed concrete failure cases: `secret()` maps keyring errors to "absent", and `store_secret` can fall back to the file. So the design adopted one vault-only record per account (§2, Alternative G). |
