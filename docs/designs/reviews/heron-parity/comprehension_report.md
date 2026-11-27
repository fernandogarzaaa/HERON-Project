Status: resolved

# Stage 6, Comprehension Test: annotated review report

**Target:** `docs/designs/heron-parity.md`, after the stage 5 fixes (commit `43d9a64`).
**Reviewer:** `egm-comprehension` (fresh context; the first try stopped on an API rate limit and was rerun fresh).
**Verdicts:**
*   Test 1, whether a newcomer could re-implement Phase 1: **Fail**. There were 4 Blockers.
*   Test 2, whether a reader understands the later phases: **Pass with gaps**.

**Resolution:** all 32 findings were checked against the code. Each was accepted, and each edit below is in the doc. None was contested.

| # | Severity | Finding | Edit |
|---|---|---|---|
| 1 | Blocker | Surface per session or per turn | Definitions: the surface comes from each turn's `Origin`. `run_turn` takes it, and the session's stored origin stays as it was created |
| 2 | Blocker | A conversation grant satisfies a rule-forced ask (`agent.rs:645`) | §2: an ask-rule match is once only, through `once_only(tool, summary)`, which `may_grant_for_conversation` already checks (`agent.rs:158` on the features base). Its card offers only Allow once and Deny |
| 3 | Blocker | The core can't know what was spoken | §3.5: `POST /api/cancel {run_id, soft, spoken_chars}`. The core cuts the run's text and adds ` [interrupted]`, or, if the run already ended, applies the cut with `Store::truncate_message` |
| 4 | Major | `delegate` bypasses `decide` | §1.2: `execute_call` checks scope for `delegate` before handing off, and the schema is offered only when `delegate` is in scope |
| 5 | Major | API and event shapes missing | New §1.10 Contracts: agent JSON, POST/PUT/import/activate/export, errors, rules JSON, chat events (`agent`, `agent_switched`, `session`), approvals, cancel |
| 6 | Major | Agent-named Settings entries conflict with PQ4 | §2: Never and Ask entries may name an agent, and go to `agents.rules`. Allow entries are global only |
| 7 | Major | "Always" from any agent widens every agent | §1.7: only the default agent's cards offer Always. Grants are keyed `grants:{session}:{agent}` |
| 8 | Major | ⌥Space doesn't match F1 | §3.2 and CUJ P-2: a shortcut the owner records in Settings › Voice, with no default. Pressing it again ends the session |
| 9 | Major | Addressing details | §1.3: longest name first; the boundary set `, : ; . ! ?`; prefixes removed; "hey" plus a name routes; refusals are a fixed stored message, with no model call |
| 10 | Major | Default agent lifecycle | §1.1: only `heron` may have `all: true` and `surfaces ["*"]`; it can't be archived; its name always comes from `cfg.agent.name` |
| 11 | Major | Soft-cancel edges | §3.5: pending approvals are cleared and treated as Deny; the existing `cancelled` event is used; `TurnEnd::Cancelled`; no learning after a cancel |
| 12 | Major | `Agent::assemble` doesn't exist | §1.5: `Agent::prompt_blocks` (`agent.rs:318`). The base block uses the agent's name, and the instructions go in the existing `PromptSource::Persona` block |
| 13 | Major | Memory paths missed | §1.4: `Agent.memory`, `count_matching` and `is_unconfirmed` are scoped. Writes stay in the agent's write pool, so an `own` agent can't change shared rows. Scope `none` refuses with a fixed text |
| 14 | Major | No unit adds the content entries | A3.6: `ContentEntry::VoiceUtterance` and its walk test. Instructions reuse `PromptSource::Persona` |
| 15 | Major | Avatar transitions | §3.2: `VoiceEvent`, the precedence order, status lines and looks |
| 16 | Major | Voice model setting | §1.10: the `[voice]` keys, local server detection, and falling back to the main model when the voice model is unreachable |
| 17 | Major | VAD and echo parameters | New §3.5a: noise floor, a 12 dB threshold, 120 ms start, the echo check, end phrases, Mute |
| 18 | Major | "G9" means two things | Goal renamed G10. Definitions add "the v1 owner trial (gate G9 in heron-v1.md)" |
| 19 | Nit | CUJ P-4's reason | The reason is now that the ask glob matches the chained command |
| 20 | Nit | F3 missing | F3 added to the preconditions; the screen falls back to "after Activity" |
| 21 | Nit | PQ2 reverses F7 | Stated in PQ2 |
| 22 | Nit | Row 12 has no phase | It now reads "P2 or later, set by that design", and the computer-use design adds the `computer` group |
| 23 | Blocker | A1 can't pass inside `src/agents/**` | A1's scope is now `src/**`: it adds `pub mod agents;` and `Origin::Voice`. `RuleGlob` types are defined |
| 24 | Major | The voice gate runner has no interface | New target `heron-voice-bench` (V1.4a). `run.sh` starts the core and runs the bench. `--self-test` needs no V1 |
| 25 | Major | Flags have no behaviour | §1.10: feature keys `agents` and `live_voice`, what each switches off, and how the app reads them (`GET /api/features`) |
| 26 | Major | PR #84 precondition unclear | Now "Not required", with the reason |
| 27 | Nit | Tooling terms and unit ids | Definitions cover skein, testteeth, genesis and model role. The Work units note explains the id gaps |
| 28 | Nit | Vague paths | `Sources/HeronUI/Views/ChatView.swift`; `Views/VoiceSettingsView.swift` as a new `voice` tab |
| 29 | Nit | Brittle counts | Changed to `-ge` |
| 30 | Nit | How the unscoped-call test works | `include_str!` over `src/tools/*.rs`, `agent.rs` and `workflow.rs`, failing on `_unscoped(` |
| 31 | Major | P2 delta count | §10: five P2 deltas, each named with its contents |
| 32 | Nit | §8 "P1/P2" | Now "today (Telegram); P2 (other channels)" |
