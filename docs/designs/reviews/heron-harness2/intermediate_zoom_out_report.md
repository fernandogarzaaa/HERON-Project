# Stage 8: Intermediate Zoom Out, heron-harness2.md

*   **Reviewer:** `egm-zoom-out` (Goldfish), on the third draft `946b7aa`
*   **Status:** resolved in the fourth draft

| # | Severity | Finding | Resolution |
|---|---|---|---|
| 1 | Blocker | Keystroke and screen text were captured, but nothing read them | **Fixed.** A fifth fixed job, topic minutes, counts the minutes of the owner's own task and project terms in each app, without the text leaving the vault. New HQ23 lets the owner defer the Keystrokes and Screen switches until a richer reader exists. Fact 5 states the limit |
| 2 | Major | `selfheal.yml` repeated `ci.yml` | **Fixed.** It now runs only R2, mutation and containment. Heron requires every PR check, `ci.yml`'s included |
| 3 | Major | The Phase 1 G7 smoke test ran model-built code on the Mac only to set a label | **Fixed.** The smoke test moved to Phase 2. In Phase 1, the only model-written code that runs locally is the repair agent's sandboxed `cargo test` |
| 4 | Major | R7, R9 and `OUTSIDE_SCOPE` were built in Phase 1 for a Phase 2 decision | **Fixed.** All three moved to Phase 2. Phase 1 takes `origin_tainted` from the tainted-session counter |
| 5 | Major | Phase 1 bundled three subsystems, and Pre-4 blocked self-healing | **Fixed.** Three graphs (1a, 1b, 1c). Pre-4 now gates only 1c |
| 6 | Major | sealbox was under-justified, and it conflicted with "no new dependencies" | **Fixed.** The non-goal now names sealbox as the one new outside tool. New Alternative N covers a Rust-side store. §8 states what sealbox gives |
| 7 | Major | HQ23's agent-query machinery was built for a switch that defaults to off | **Fixed.** Removed (no `serve`, no `--tools`). It is now listed under Future Improvements |
| 8 | Major | There were two connector registries, and the `put_api_key` reuse failed | **Fixed.** Manifests are now the execution layer for the existing `rest_api` transport. `require_custom` accepts manifest IDs |
| 9 | Major | `offer_connector` duplicated the typed-report path | **Fixed.** Replaced by the report sheet's "Request a connector" mode. Proactive offers are listed under Future Improvements |
| 10 | Nit | Recorded pairs only checked the model against itself | **Fixed.** They are now request-shape pairs, and the doc says plainly that they test the template, not the API |
| 11 | Nit | The first-use counter duplicated MCP's trust model | **Fixed.** Connectors now use MCP's model: untrusted until the owner trusts them on the card, and trust resets when the manifest's hash changes |
| 12 | Nit | The HTTP reuse claims didn't match `web.rs` | **Fixed.** The scout has its own client (`PublicOnlyResolver`, `no_proxy`, an allowlist redirect policy). The executor uses `PublicOnlyResolver` |
| 13 | Nit | There were two file-observation pipelines | **Kept, with the reason given.** Paths outside the working folder are more personal, so they go to the vault |
| 14 | Nit | A second Swift client, and inconsistent checks | **Fixed.** `CoreClientProtocol` is extended instead. All checks use `require-tests.sh` |
