# Stage 6, Comprehension Test: heron-auto-router.md

Reviewer: `egm-comprehension` (Goldfish). Target at commit ccd395f. Verdict: both ranges failed the re-implementation standard (3 Blocker, 13 Major, 9 Nit). All findings were accepted and fixed.

| id | finding | fix |
|---|---|---|
| F1 (Blocker) | No source for tokens in/out per model or wall time; the run record holds one `tokens` total (`eval/run_core_tasks.py:459-461`) | E0 adds `wall_ms` and a per-model `models` split read from each run's own `heron.db` `usage` table |
| F2 (Blocker) | "A tool call returned an error" undefined | Defined in Terms; `execute_call` returns a `CallOutcome`; only `ToolFailed` counts, so declined approvals don't escalate |
| F3 | Step indexing and error counting | Steps count from 0; errors count since the turn's last move; one move per step |
| F4 | Tainted floor vs rules 1 and 3 | The floor applies to rule 5 only; RQ3 and the test say so |
| F5 | G1 and the gate set different bars | One definition: pass rate over runs, tolerance of 3 runs, cost at most 85% of every baseline that matches |
| F6 | An equality verifier can't give under/over | The verifier writes `correct` (genesis) and `direction` (metrics) |
| F7 | No schemas for `labels.json`, `replay.json`, case text | Given in §5.2 |
| F8 | codex empty model, alias equality, the id list | Defined in §1 |
| F9 | Pin API responses, errors, `tiers` shape, event values | Defined in §4.1 and §4.2 |
| F10 | "G9" meant two things | RQ4 names heron-v1's G9 owner trial; Terms define it |
| F11 | EVE drives HTTP, not the Mac UI; no task file | `eval/eve/router.yaml` over `/api/chat`, written in E0 |
| F12, F13, D5 | Undefined terms | A Terms block links heron-v1.md and heron-v1-features.md and defines the build terms |
| F14 | "As today" claims too loose | CUJ 7 lists what changes for the HTTP provider |
| F15 | PT-R2's mismatch log built by nobody | Record only |
| D1 (Blocker) | R1 couldn't read `deep_model` or move `looks_simple` within its scope | Pre-steps add the `deep_model` field; R1 copies `looks_simple` and R2 deletes the old one |
| D2 | `usage::route` wrapper contract | `route` is deleted; R1's tests cover what its test pinned |
| D3 | Delegate fixed pin mechanism | `Session::ephemeral` takes a `model_pin` |
| D4 | Rust `Event::Model` matches break; `fast_model()` borrows a temporary | Matches gain `..`; `fast_model()` returns `String` |
| D6 | `Session.model_pin` missing | Added, filled by `Session::load` |
