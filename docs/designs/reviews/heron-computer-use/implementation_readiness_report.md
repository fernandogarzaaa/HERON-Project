# Stage 9, Implementation Readiness: heron-computer-use.md (plan)

Reviewers: `egm-implementation-readiness` (5 Blocker, 9 Major, 6 Nit), `egm-codebase-details` (6 Blocker, 15 Major, 6 Nit), `egm-style` (4 Major, 13 Nit). Target at commit fe92a72. The Detailed Implementation Plan and Work units were rewritten, with spec changes to §1, §2, §2.1, §3, §4.1 and §7; every finding below was accepted.

| finding (source) | fix |
|---|---|
| Blocker: C2 waited for PR #84 W1/W2, but the `execute_call` wiring is W6a | Pre-step 3 and C2 depend on W6a; C2's check greps `src/agent.rs` for `RepeatGuard` and `journal::guarded` |
| Blocker: C1's tools couldn't reach the permit, bridge or run id (`FnTool` is a plain fn; `ToolCtx` lacked them) | Pre-step 4 adds `computer`, `offers_computer` and `run_id` to `ToolCtx` and every literal, through a `ComputerPort` trait C1 implements |
| Blocker: a new `ToolCtx` field broke `tests/integration.rs:350` outside C2's scope; the settings fixture would break C3 | The pre-step updates every literal, that one included; C3 re-records `settings.json` |
| Blocker: `/ready` couldn't be tied to the app's stream; reconnects unhandled | `GET /api/events?driver=<id>`, a drop guard on the SSE response, a 5 s grace, `onReconnect` re-posts `/ready`, `GET /api/computer/pending` after a resync |
| Blocker: no taint field for the sheet | `tainted` on the `start` action |
| Blocker: double wrapping broke the untrusted block and taint | Tools return bare text; `execute_call` wraps once and sets taint |
| Blocker: drive cards had no storage | Two nullable `suggestions` columns in a schema step (Q7 now names `store.rs`); accept returns `{prompt, session_id}` |
| The permit slot's home; `run_parked` bypassed the lock | Permits live in `ComputerBridge`, keyed by session; tools and `execute_call` refuse without `offers_computer`, which `run_parked` never sets |
| `offers_computer` only hid schemas | `execute_call` and each tool refuse `computer_*` without it |
| Concurrency of `LocalRead` computer tools | A step with a computer tool runs in order |
| The repeat nudge would fire on legitimate repeated keys | Computer tools skip it; `RepeatGuard` compares outputs |
| `offered()` had no mode; `Observation` lacked `window_count`; `ErrorCode` open | Signature takes `mode`; types match §2.1; unknown codes decode as `action_failed` |
| Unlisted Rust and Swift files, exhaustive switches, settings tab, wiring into `HeronApp.swift` | All listed |
| Fixture app had no bundle ID; ElementTree fixtures preceded their format | `app.heron.drivefixture` via a wrapper script; fixtures written by hand from §2.1, recorded trees later |
| The disclaimed launch was undesigned | Unit C4b, built only if PT-C7 requires it: a C shim, a `Launcher` protocol, a fallback |
| Missing text: error sentences, prompt block, tool descriptions | All given in item 1 and item 3 |
| Observation trimming undecided | Per-request copy only; identified by `tool_call_id`; app name from the first line |
| Loop bound can't change in a `for` range; `u32` vs `usize` | A `while` loop with a variable bound; `usize` |
| Checks named only some tests; substring Swift grep | Every check names every test; one anchored `grep -cE` count per Swift unit |
| Style: wrong `register_resumer` claim; duplicate test names; "should" without a check; drop guard unassigned | Fixed; the drop guard is `PermitGuard` in `agent.rs` |
| Nits: SystemDesktop test hook, snapshots, `paused` not a code, fixture counts, wording | `SystemDesktop` moved to the `Heron` app target, which tests don't link; snapshots recorded and committed by C5; wording fixed |
