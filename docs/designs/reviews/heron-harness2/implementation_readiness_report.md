# Stage 9: Implementation Readiness, heron-harness2.md

*   **Reviewers:** `egm-implementation-readiness`, `egm-codebase-details` and `egm-style` (Goldfish), working in parallel on lines 1047-1487 of `2a80442`
*   **Findings:** 10 Blockers, 50 Majors and 21 Nits across the three reports, with heavy overlap
*   **Status:** resolved in the fifth draft. The implementation plan was rewritten, and the feature sections were patched. Stage 9 was **not re-run** after the fixes; see [Residual](#residual).

## Blockers

| Finding | Reviewers | Resolution |
|---|---|---|
| H1 needs H2's config, `add_inbox` action and taint counter | IR B1, CD B1 | Moved into the Pre-6 skeleton: config structs, `to_v4`, `add_inbox_with_action` and `add_suggestion_with` (with the old functions kept as wrappers), `assemble_with_tools`, `tainted_sessions()`, `PromptSource::Repair`, the content entries, and `is_outside` |
| H2's clippy breaks `tests/integration.rs`'s `add_inbox` call | CD B2 | The new insert functions are added alongside the old ones, which stay as wrappers. No existing caller changes |
| The gate had no arguments, no `base` source, and a depth-1 checkout | IR B2, style 1 | Item 26 is now exact YAML: `fetch-depth: 0`, base as `merge-base origin/main HEAD`, and the exact `run:` line. Item 21 gives the script's signature |
| The `selfheal.yml` branch filter would never fire | CD B3, IR M6, style 10 | Plain `on: pull_request`, plus `if: startsWith(github.head_ref, 'heron/selfheal/')`. `poll_ci` requires the `selfheal / gate` check to be present |
| Containment `check.py` only re-reads committed records | CD B4 | Dropped from the CI gate. It is now listed under "What the gate does not prove", and the `cli_llm` containment unit tests still run in `ci.yml` |
| The 1b and 1c graphs couldn't reach 1a's output | IR B3, style 5 | The graphs now run in sequence, each merged to `main` before the next is cut (new "Graph order" section). `is_outside` moved to Pre-6 |
| No Mac connectors UI, and no trust route | IR B4 | New `POST /api/connectors/custom/{id}/trust`, H9's `ConnectorsSettingsView` with its client calls and two tests, and H7b docs |
| The genesis commands had no `--suite` and used the wrong verdict word | IR B5, CD M7 | `--suite code-py`, a `genesis_verifier.py` adapter, and `TRUSTED`. The undefined "fence claim" was dropped, so the fence's evidence is its mutation score |

## Majors and Nits (grouped)

*   **Fixtures and decoders.**
    *   Re-record `state.json` and `connectors.json`.
    *   Add `ModelDecodingTests` decoders.
    *   Add `ManagedCoreClient` forwarders.
    *   Touch `Wire.swift`, `InboxStore` and `CoreProcess`.
*   **Contracts.**
    *   `check-fence` output and exit codes.
    *   The `report`, `replay` and `record-replay` subcommands, on the standalone path.
    *   `GET /api/inbox` `action`.
    *   The checklist items.
    *   The token route.
    *   `HERON_PREVIOUS_EXIT` for `core_exit`.
    *   `commit` taken from `option_env!`, as `null` when unset, with a null commit giving `setup_failed`.
*   **Correctness.**
    *   **Rollback.** `to_v4` raises `user_version`, and an older build refuses a newer database. Rollback now restores the pre-migration snapshot.
    *   **Error shape.** Errors keep the real shape, `{"error": text}`.
    *   **`internal_error`.** Dropped, because every handler error becomes a 400 and would trigger repairs for user input.
    *   **Tick.** It runs inside `own_background_work()`, every third 20-second pass.
    *   **Topic terms.** The doc now says plainly that model-made task titles can be terms, and that a term only selects what is counted.
*   **Connectors.**
    *   Manifests are merged into `connectors::list()` and never written to `connectors.json`.
    *   The name regex is `{2,30}` and can't collide with a catalog key.
    *   The executor reimplements the host rules, and H8w switches `connectors::validate` over to them.
    *   There is a vault-only `api_key` and `sources::is_known`.
*   **Observation.**
    *   The exact `Profile::Observe` SBPL and its order. A spawn is refused when `wrap` returns `None`.
    *   A fake sealbox fixture via `HERON_SEALBOX_BIN`.
    *   macOS-only `cfg` gating, with 501 responses elsewhere.
    *   Seam protocols for the observers.
*   **Build and CI.**
    *   The `build-app.sh` argument loop and what `--host-arch` does.
    *   A `find`-based connectors copy.
    *   H5's check now runs the build.
    *   cargo-mutants and testteeth are pinned with hashes.
*   **Checks.**
    *   `required-tests.txt` lines are `<graph> <selection> <name>` and include item 18. `check-required.sh` runs them.
    *   The Swift test class files are named.
    *   H8t has named tests and a fixture.
    *   Clippy is added to H8, H8w and verify-1c.
    *   verify-1b runs `swift test`.
    *   `harness2::` is used as a module filter.
    *   H4 runs a real R2 test (`test_verify_selfheal.py`).
*   **Prototypes.** PT1 has a pass threshold. PT4 is recorded only. The doc says which graph needs which prototype. HQ10 is added to Pre-3, and the threshold is in config.
*   **Citations corrected:**
    *   `cli_llm.rs:38-42`
    *   `schedule.rs:250-257`
    *   `mcp.rs:204-211` (pinning, not a trust reset)
    *   sealbox `sandbox.py:62-66, :119`
    *   the `check_workspace` attribution, which now reads: writes outside the worktree are `Privileged`, and allow rules never cover them
    *   `DenyAll`, which is already public, so that instruction was dropped
*   **Style.**
    *   A naming key at the top of the plan.
    *   One private-text definition: added lines, normalised.
    *   Item numbers documented as stable IDs.
    *   Full paths everywhere.
    *   "1 day, 3 days or 1 week".

## Residual

*   **Stage 9 was not re-run after these fixes.** The rewrite is large, so a fresh `egm-implementation-readiness` pass on the final draft is the recommended first step after the owner's answers. Under EGM's one-re-audit cap, that pass would be this stage's only re-audit.
*   **Unchecked claims.** Two claims have no tool run behind them: the cargo-mutants install line, and the GitHub `head_ref` filter behaviour. PT6 and the first real self-heal PR check them.
