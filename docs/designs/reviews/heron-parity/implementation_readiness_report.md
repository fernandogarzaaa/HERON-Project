Status: resolved

# Stage 9, Implementation Readiness Test: annotated review report

**Target:** `docs/designs/heron-parity.md`, the Detailed Implementation Plan and Work units (lines 1122–1404 at commit `09156c5`).
**Reviewers:** `egm-implementation-readiness`, `egm-codebase-details` and `egm-style`, each in a fresh context.

| Reviewer | Verdict | Blockers | Majors | Nits |
|---|---|---|---|---|
| Readiness | not ready | 2 | 17 | 8 |
| Codebase details | not ready | 4 | 21 | 4 |
| Style | — | 2 | 14 | ~18 |

**Resolution.** The plan and the Work units were rewritten, and the feature sections they depend on were corrected. Every finding was accepted. None was contested.

**A stage 8 regression, fixed here.** One stage 8 edit had removed the delegate, enforcement and memory paragraphs from §1.2. They were restored from commit `b85b10e`, and a heading diff against that commit confirms nothing else was lost.

## Blockers

| Finding | Edit |
|---|---|
| Readiness B1 / codebase M19: genesis spec undefined, wrong suite | V2 item 4 now follows `eval/genesis/limit-classifier.yaml`. It defines the dataset, a builder for 14 named cases, a verifier that accepts `{"pass": bool}`, the `task_success >= 0.90` metric, and `--suite json` |
| Readiness B2 / codebase M12: NFKC needs a crate outside A1's scope | NFKC was dropped. Names are unique by `to_lowercase`. Lookalike names across scripts are a stated limit, and approval cards show the avatar and id |
| Codebase B1: the newer-format refusal already exists (`store.rs:609-616`) | The duplicate item and test were removed, and the downgrade NFR was reworded around the existing guard |
| Codebase B2 / style B2 / readiness M2 / codebase M15: the rules stack ignored `/api/nevers` and contradicted itself | `/api/nevers` and the Never section are unchanged. New `/api/ask-rules` routes are built the same way and write through `update_config`. A new `AskRulesSection` sits beside `NeverSection`. Agent rules travel only in the agent JSON |
| Codebase B3 / readiness M17: `heron-cli` isn't a cargo binary | `run_gate.py` reuses `Core` from `eval/run_core_tasks.py` (`heron serve`) in a temporary `HERON_HOME`. The bench runs inside the built app bundle, so it gets the microphone and speech permissions |
| Codebase B4 / readiness M9: new fixtures break `ModelDecodingTests` | A4 item 2 adds the decoders. A3t lists the changed and new fixtures, the way to re-record them, the test-config keys, and the `tests/integration.rs` compile fixes |
| Style B1 / readiness M11: V1 builds the P4 layouts | V1 item 5 builds the docked panel only |

## Majors (grouped)

*   **Approval flags** (codebase M1, readiness M7): `ApprovalRequest.once_only`. `can_always` is false for `once_only` and for non-default agents. The server, terminal and Telegram approvers check it, and `ApprovalSheetView` is in A4. Test `named_agent_card_never_offers_always`.
*   **Live policy** (codebase M2, M3): `decide` reads the global ask rules from `ctx.store.live_policy()`, so `Policy::new` keeps its signature and background runs are covered (`ask_rule_applies_in_job_and_proactive_runs`). **Always** writes to the live policy (`always_rule_lands_on_live_policy`). The deny mirror functions that ask copies are listed.
*   **`run_turn`** (codebase M4, readiness M4): new signature with `TurnInput` and `TurnEnd`, and every caller listed. A soft cancel keeps the run in `running`. `run_chat` publishes `cancelled` and `run_end` and skips learning.
*   **`prompt_blocks`** (codebase M5): named, with the features-base line shift noted.
*   **Memory pools** (codebase M6, M7, M8; readiness M5): a `PoolFilter` on every listed store function. Owner callers pass `All`. `Memory::new` is shared-only, so `App.memory`, MCP, schedules and Telegram read only the shared pool. `pool_all_not_used_by_tools` lists its nine files.
*   **Voice provider** (codebase M9, readiness M13): `HttpLlm::from_voice`; `HttpLlm::warm`, which bypasses the 2048-token clamp; wrapped in `SwappableLlm` and the meter; falls back to the main model once.
*   **Message storage** (codebase M10, readiness M1): `messages.agent` and `messages.run_id` are columns, not `Message` fields, so providers never see them. `truncate_message(session_id, run_id, spoken_chars)` is defined.
*   **Audit v3** (codebase M11): hashing of a null agent and the field order are defined; the `delegated` decision is named; the two existing tests to update are named.
*   **Content entries** (codebase M13): `AgentInstructions` is not a content entry, because it uses `PromptSource::Persona`. `VoiceUtterance` has `taint_kind` none.
*   **Chat stream** (codebase M14, readiness M3): one `session`-event rule. `address()` runs before the session loads, and `sessions.from_session` keeps the link back.
*   **Mac file lists** (codebase M16, M17, M18; readiness M10, M11, M12): the full HeronCore client and protocol files, `ChatAndApprovalsStores`, `ApprovalSheetView`, the Memory filter, `visibleDestinations` with the ScreenStoreTests fix, the Settings voice tab case, the entry points, the shortcut recorder, the probes, the queue, and bench mode in `HeronApp.swift`.
*   **Tool groups** (codebase M20): `connectors` dropped from P1. `mcp` expands to `mcp__x__*`. `expand` uses `names_for`.
*   **Signatures** (codebase M21, style M3): one `effective_scope(global_mode, agent, surface)`.
*   **Missing tests** (readiness M14): refusal, feature-off, `heron` delete, scope none, voice fallback, soft cancel during approval, `use_skill`, and others, each named in the A3 check.
*   **Ownerless failures** (readiness M15): every Rust unit runs clippy `--all-targets`. `verify-p1` includes both builds. A failing verify gets a fix node.
*   **Swift checks** (readiness M16, style M9): each required test name is checked by name.
*   **Style majors M1–M14:** P0 matches the Preconditions; the cancel body is complete; `features.rs` is its own item; `Origin::Voice` is added once (A1); `once_only` and `heard` are separate bullets; steps are named 2b and 4b; V1 heading and paths are fixed; the voice activity detector text matches §3.6; there is one `voice-timing.jsonl` rule; "G5's latency bound" is used; the G4 proof is a single list.

## Nits

All applied:

*   line drift (`cli_llm.rs:995`, `sandbox.rs:500`);
*   the `ChannelKind` list;
*   the `eval/voice/__init__.py` import;
*   V1p's file and strings, checked with `plutil`;
*   the `scripts/gates/parity-p1` label;
*   snapshot references go to the owner for review;
*   the PR #75 fallback test name;
*   uniform item references in the table.
