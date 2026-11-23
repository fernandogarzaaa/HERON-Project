# Stage 9, Implementation Readiness: heron-auto-router.md (plan)

Reviewers: `egm-implementation-readiness` (1 Blocker, 13 Major, 9 Nit), `egm-codebase-details` (1 Blocker, 14 Major, 8 Nit), `egm-style` (5 Major, 10 Nit). Target at commit fe92a72. The Detailed Implementation Plan and Work units were rewritten; every finding below was accepted.

| finding (source) | fix |
|---|---|
| Blocker (both): adding fields to `Event::Model` breaks `tests/integration.rs:651`, outside R2's scope | Pre-step 2 adds `..` there, and adds `ToolCtx.turn_tier` to every `ToolCtx` literal, including `tests/integration.rs:350` |
| The existing routing integration test goes red (`:621-702`) | R3 rewrites it, with the new model list and reasons; it is in R3's check |
| README and `assets/index.html` outside every scope | README is a post-V build-session step (pre-step 8); the web Usage page is unit R5 (`assets/**`) |
| Saving the web or Slint Usage tab turned routing off; `main_model` comparisons broke | `main_model` stays a raw id; `main_model_name` and `routing_available` added; both UIs and `POST /api/usage` stop tying routing to `fast_model` |
| "resp.model differs" fired on every step | `ChatResponse.fell_back`, set only by a real fallback; `ScriptedLlm` echoes `req.model` |
| The model-equality rule was wrong (`claude_native_model` keeps the user's spelling) | A defined model key; `model_key_matches_every_spelling` |
| The CLI fallback named the wrong function and had no unknown-model detector or streamed-text guard | Typed `CliLimit` and `CliUnknownModel` errors; retry in `CliLlm::chat` with the standard id, never after streamed text; three tests |
| R4 read R3's fixtures without depending on R3; Mac files and test files unlisted | R4 depends on R3; all files named; default associated values keep existing tests compiling |
| The new-chat pin flow was undesigned | `POST /api/chat` takes `model_pin`; the menu reads `CoreSettings.tiers` before the first message |
| G and L could edit what judges them; tuning undefined | G's scope is `eval/router/replay.json`; L reads only; both stop and report on failure; tuning declared out of scope |
| E0 lacked the live modes; runner details (tasks file, fixtures, driver, artifacts) | Item 1 specifies `--setup router`, `--gate --live`, a `tier` field, `--fixtures-root`, `--driver direct`, separate folders |
| New record fields broke `json_verifier`'s strict keys | `RECORD_KEYS` gains three optional keys; schema stays 1 |
| The verifier output departed from the genesis contract | Writes `{"pass","score","reward"}` plus `direction`, following `limit_verifier.py` |
| The replay test path and parser couldn't be checked | `router::replay::tests::replay_cases`, a non-ignored inline parser test, R1 depends on E0, G checks `replay.json` exists |
| Rule helpers left open (reason order, path and requirement definitions, `HEAVY` matching) | A reason table, regex-level definitions, `HEAVY` keeps substring matching |
| `typed` plumbing; delegate's access to the tier; `execute_call` callers; shell exit detection | A private `run_turn_inner` parameter (origin rejected, with reason); `ToolCtx.turn_tier`; callers listed; a shell flag instead of text matching |
| Schema step, schema tests, rollback claim | `to_v4`, both store tests named; rollback needs `heron restore` |
| Checks named only some tests | Every check lists every test its items name |
| Style: `fast_model()` signature stated three ways; "replaces the body" vs deleted; `deep_left` sentence; "should" without a check | All reconciled; testteeth's 0-survivor aim stated as an aim |
| Nits: config comments, notice text per tier, `deep_model` raw id, regenerating `router-cases.jsonl`, clippy and swift build in V, wording | Fixed |
