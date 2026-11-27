Status: resolved

# Stage 8, Intermediate Zoom Out: annotated review report

**Target:** `docs/designs/heron-parity.md` (full file, commit `b85b10e`)
**Reviewer:** `egm-zoom-out`, in a fresh context. It read the doc, the earlier stage reports and the cited code.
**Verdict from the reviewer:** changes required. It found 1 Blocker, 8 Majors and 3 Nits. Most of them are complexity that the stage 6 and stage 7 fixes added.
**Resolution:** all 12 accepted, and the edits below are in the doc. Phase 1 is now smaller than it was after stage 7.

| # | Severity | Finding | Edit |
|---|---|---|---|
| 1 | Blocker | The audit names the agent twice: a column in §1.7, and a `decision` suffix in A3.6 | The suffix line was removed. A3.6 now points to the `agent` column, and CUJ P-1 step 3 says "the audit row's `agent` column" |
| 2 | Major | `Verdict::AskOnce` duplicates `once_only` from `heron-v1-features.md` B2 | AskOnce was removed. The ask step returns `Verdict::Ask`, and `once_only` becomes the `Policy::once_only(tool, summary)` method, which is also true when an ask rule matches. B2 already wires `once_only` into grants, `can_always` and the approvers |
| 3 | Major | Rules are kept in three stores, edited from two screens and checked in two places | Agent rules are edited only in the agent editor and merged into the turn's `Policy` lists. `/api/rules` and Settings › Rules are global only, and the cross-store transaction and `Scope.rules` are gone. `Policy::new` keeps its explicit ask argument: reading the live policy wasn't confirmed to cover the MCP server's policies |
| 4 | Major | The three voice switches restate step 6 of `decide` | They are replaced by one `heard` flag, which `decide` treats like `tainted` at step 6. `may_grant_for_conversation` gives no grants under it. It doesn't taint the session. The voice mode ceiling was dropped from P1, and ceilings stay a P2 channel concept |
| 5 | Major | Per-agent grant keys miss `sandbox::read_grants_from` | Dropped. With one agent per thread, `grants:{session}` already belongs to one agent |
| 6 | Major | Switching agents inside a thread belongs in a later delta | P1 has one agent per thread: addressing another agent opens a new thread with a link back. `agent_switched` and the pool-readability rule were removed, and PQ3 states the cost. In-thread switching moves to the P2 channels delta. "Learning stays with the speaker" is now described as existing behaviour (`agent.rs:819-831`), guarded by a test |
| 7 | Major | Sending profile facts to the shared pool reopens stage 5's B1 | That rule was removed. PQ6 now makes `shared` the default memory scope for new agents, with `own` and `private` as opt-ins |
| 8 | Major | Export and import should move out of P1 | They move to the P3 small-business pack delta (matrix row 1c). These were removed from P1: `share.rs` and its tests, the `review` state, `ImportedInstructions`, CUJ P-7, the import, activate and export routes, and the review sheet. §1.8 keeps the safety requirements that delta must meet |
| 9 | Major | PQ2 and R9 (approving by voice) should move out of P1 | Voice never approves in P1, so F7's rule stands. R9 moves to the Direction group, and the PQ2 branch in §3.5 is gone |
| 10 | Nit | `propose_agent` has no P1 user | Deferred to the P2 goals delta |
| 11 | Nit | Three voice-panel layouts | P1 ships the docked layout only. The pill and full-window layouts come with the P4 avatar work |
| 12 | Nit | `spoken_chars` is tracked per word | Now tracked per sentence, in the last assistant message: the offset at the end of the last sentence whose speech started |
