Status: resolved

# Stage 7, Critic Review: annotated review report

**Target:** `docs/designs/heron-parity.md`, feature range (lines 1–1055 at commit `4183dae`).
**Reviewers:** `egm-design`, `egm-structural` and `egm-style`. Each ran in a fresh context with only the doc, its persona and standard, and the cited paths.

**Counts:**

| Reviewer | Blockers | Majors | Nits |
|---|---|---|---|
| Design | 3 | 14 | 6 |
| Structural | 0 | 11 | 12 |
| Style | 0 | 13 | 15 |

**Resolution:** every finding was checked against the code, and each edit below is in the doc. No finding was contested. Some findings were accepted but deferred, or fixed only in part:

*   Structural nit 15 (where Definitions sits) is kept as is, because the template has no Definitions section.
*   Structural nit 19 (the channels table breaks its list) is kept, because indenting the table would change how it renders.
*   Design nit 23's never-rule reach was answered with a pointer to PR #84's egress allowlist rather than a new mechanism.

## Design critic

| # | Severity | Finding | AI suggestion | Edit |
|---|---|---|---|---|
| 1 | Blocker | A named agent escapes its scope through `schedule_add`: jobs run as the default agent (`app.rs:1543-1553`) | PROPOSE FIX | `schedules` and `admin` are default-agent-only in P1, and `validate` refuses them for others. CUJ P-1's Mango loses `schedules`. Test `schedules_and_admin_refused_for_named_agents` |
| 2 | Blocker | Memory leaks through the shared thread history (`agent.rs:518`) and through learning | PROPOSE FIX | PQ3 is generalised: a switch stays in the thread only if the new agent can read every earlier agent's write pool; otherwise a new thread opens. Learning reads only the current agent's reply. CUJ P-1 step 5 is updated. Tests `history_of_other_agent_absent_from_request` and `learning_reads_only_current_agent_reply` |
| 3 | Blocker | Ask rules miss background policies, and a conversation grant satisfies them | PROPOSE FIX | `Verdict::AskOnce`, which grants never satisfy. `Policy::new` takes the ask rules, and every constructor listed (jobs, proactive, social, MCP) passes them. Tests `every_policy_constructor_gets_ask_rules` and `ask_once_ignores_conversation_grant` |
| 4 | Major | A mode ceiling isn't "always asks" (allow rules, egress in readonly, state writes) | PROPOSE FIX | The `voice` surface adds `ignore_allow_rules`, `ask_state_writes` and `ask_egress`, and §3.7 is corrected. Tests `voice_surface_*` |
| 5 | Major | Imported instructions are Trusted | PROPOSE FIX | `ContentEntry::ImportedInstructions` (Outside): the session starts tainted until the owner edits or trusts the instructions. Hidden Unicode characters are refused. CUJ P-7 is added |
| 6 | Major | `search_knowledge` sits in `memory` | PROPOSE FIX | A new `knowledge` group, off by default |
| 7 | Major | The default `own` scope fragments the owner's facts | PROPOSE FIX | `profile` and `preference` facts from `own` agents go to the shared pool. The trade-off is stated |
| 8 | Major | Agent ids can be reused | PROPOSE FIX | Ids are name plus 6 random hex digits and are never reused. Names are freed on archive. Test `agent_id_never_reused` |
| 9 | Major | Rollback, and the key off, expose private pools | PROPOSE FIX (partial) | Accepted risk stated for pre-P1 builds. From P1 on, a newer data format is refused. With the key off, the default agent still reads only the shared pool. Alternative I records the per-table option |
| 10 | Major | Barge-in conflicts with 409, false triggers, approval waits | PROPOSE FIX | Utterances are queued until `cancelled` or `done` ("One moment" after 1.5 s; hard cancel after 30 s). Barge-in needs a partial transcript of two letters or more. The path while an approval waits is defined |
| 11 | Major | `spoken_chars` counts queued text, not text heard | PROPOSE FIX | `willSpeakRangeOfSpeechString` gives the heard text, and `SentenceSplitter` keeps the original text ranges. The offset spans all of the run's messages. Test `spoken_chars_cut_spans_messages` |
| 12 | Major | The latency budget is unjustified | PROPOSE FIX | A budget table to first sentence; a slim, cacheable voice prompt; `POST /api/voice/warm`; and a stated result if G-V fails |
| 13 | Major | Recogniser behaviour unverified | PROPOSE FIX | `addsPunctuation`, `contextualStrings`, refusing when on-device recognition is unavailable, the mid-session failure state, exact matching only |
| 14 | Major | Alternatives missing for P1 decisions | PROPOSE FIX | Alternatives H–M (typed rules, per-pool tables, hard abort, Whisper, neural speech, model voice detection) |
| 15 | Major | Matrix gaps (G9) | PROPOSE FIX | Rows 2c (replying while the owner talks) and 8a (Spaces); row 12 is P2; row 2b's owner is `heron-avatar.md`; the P3 deltas are named |
| 16 | Major | Messages self-chat echo loop (direction) | PROPOSE FIX | The channels row records sent message ids and skips them |
| 17 | Major | Hand-off checkout one-click buy (direction) | PROPOSE FIX | Purchase-confirming clicks are payment-class, and the browser profile holds no saved payment methods |
| 18 | Nit | The audit suffix breaks the Slint mappings | PROPOSE FIX | Audit column `agent` with hash version 3; `decision` values unchanged |
| 19 | Nit | The cancel body | PROPOSE FIX | `{"ok": true, "cancelled": bool}` |
| 20 | Nit | `update_persona` for other agents | PROPOSE FIX | Covered by `admin` being default-agent-only |
| 21 | Nit | Which policy gets new rules | PROPOSE FIX | "Always" goes to the live `app.policy`; Settings rules apply from the next model step |
| 22 | Nit | Id and name gaps | PROPOSE FIX | ASCII fallback `agent-…`; NFKC and case-fold uniqueness |
| 23 | Nit | Misc (G9 skip, choice 3, the noise floor overlapping the echo check, atomic PUT, never-rule reach, relay auth) | PROPOSE FIX | Goals renumbered; the noise floor is measured before speech; PUT order defined; the never-rule reach is answered by PR #84's allowlist (CUJ P-4); relay auth is left to the relay delta |

## Structural critic

| # | Severity | Finding | Edit |
|---|---|---|---|
| 1 | Major | Broken `#6-the-relay` anchor | Fixed to `#6-the-relay-direction` |
| 2 | Major | P0 and Preconditions disagree | The P0 row now points to the Preconditions list (with F3; PR #84 not required) |
| 3 | Major | Open Questions at H3 | Now `## Open Questions` |
| 4 | Major | PQ2 is a hidden reversal; the count is wrong | R9 (partial) added; "R4, extended" folded into R4; the Overview says nine |
| 5 | Major | CUJ P-4 and P-6 lack Given | Given added |
| 6 | Major | Givens omit the feature keys | P-1, P-2 and P-7 say the keys are on |
| 7 | Major | G6 has no verification | `test_recognitionRequestIsOnDevice` and `test_noAudioWrittenToDisk`, plus a G6 proof line |
| 8 | Major | Goals mix phases; G8 has no check | G8 tagged "P2 and later; each owning delta verifies it" |
| 9 | Major | G1 import has no CUJ; the G3 proof list is thin | CUJ P-7 added; a G3 proof line lists 5 tests |
| 10 | Major | The matrix fails the "nothing dropped" goal | See design 15 |
| 11 | Major | No alternative for ask rules | Alternative H |
| 12–23 | Nit | Separators, G9 gap, accounts table, Definitions placement, reversal order, row order, link consistency, list break, numbered Then, Overview count, blank lines, heading 3.5a | Separators added; goals G1–G9; rows 1d/1e reordered; CUJ P-4 numbered; Overview says three; 3.5a renumbered to 3.6. Kept: Definitions placement and the channels table indent (rendering) |

## Style critic

| # | Severity | Finding | Edit |
|---|---|---|---|
| M1 | Major | "two points" lists three | "three points" |
| M2 | Major | `none` means opposite things | Ceiling value renamed `unlimited` |
| M3 | Major | block, deny and never | "never rule" throughout; "deny" only as the code name |
| M4 | Major | "agent card" has two meanings | "agent payment card" for the payment card, with a Definitions row |
| M5 | Major | G8 contradicts §8 | G8 now covers actions toward others; "Outward action" defined |
| M6 | Major | "Gap N" undefined | Defined above the matrix |
| M7 | Major | "the brief" | "the owner's scope message" |
| M8 | Major | delta, F-ids, Harness2 undefined | Definitions rows added |
| M9 | Major | "V1 build" | "Work unit V1" |
| M10 | Major | Relay key ambiguity | The private key stays in the Keychain, stated plainly |
| M11 | Major | Alternative D claims G5 is met | "expected to meet G5 … gate G-V confirms it" |
| M12 | Major | Risk row puts "hey" after the name | Fixed |
| M13 | Major | delete versus archive | "`DELETE` archives" used consistently |
| N1–N15 | Nit | Counts, competitor names, labels, pool terms, phrasing, Owner doc, relay delta, RelayEvent class, `guest_replies` link, thread, dense sentences, wording | Applied where they touched a decision: the R count, Owner doc, the RelayEvent class, the Thread definition, CUJ labels, UNCONFIRMED, `["*"]`, the PR #75 wording, the owner's name. Subjective adjectives were trimmed where they made a claim |
