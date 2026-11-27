# Stage 8, Intermediate Zoom Out: heron-computer-use.md

Reviewer: `egm-zoom-out` (Goldfish). Target at commit e9d72cd. Verdict: revise (0 Blocker, 3 Major, 5 Nit). The core shape (driver in the app, permit before the first observation, the app as the only authority, harness-enforced rules) and Stage 5's cuts held; the mid-turn rebuild, variable step bound, drop guard, PT-C7, chord and word lists and hit test were judged necessary. All findings were fixed.

| id | finding | fix |
|---|---|---|
| M1 | Crash recovery built on PR #84 action entries, which PR #84's own OQ11 recommends removing; the chain turned one stray approval into one nicer approval | Dropped the action entry, the `computer` resumer and the old CQ11. Computer calls stay in `journal::guarded`; the one parked call fails safely with `no_permit`; `computer_act` answers `no_permit` at once without a live permit |
| M2 | `/api/computer/hold` and the 30 s wait duplicated the stream-close liveness signal | Removed. An action waits for the answer, stream close (`app_disconnected`), permit expiry or cancel |
| M3 | Restated values drifted (CUJ 2's 10 minutes, EVE's two prompts, route counts, the C2 step-bound text) | Each value now stated once and referenced |
| N4 | The core mirrored the app's step count | `steps_used` removed from the wire and the core's `Permit` |
| N5 | The `x-heron-client: mac` header protected nothing | Removed; any `/api/chat` turn with the app connected is offered `computer_start`, approved only in the app |
| N6 | The two-observation rule had no owner | Moved to `src/agent.rs` message assembly (C2), beside `compact_if_needed` |
| N7 | Card outline and screen frame were extras | Both removed; the pill and menu bar icon remain |
| N8 | "The phone and Telegram" are one surface; CQ order | Fixed; the old CQ12 is now CQ11 |
