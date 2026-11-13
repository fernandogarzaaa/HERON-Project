# Test strategy

## What exists (run with `cargo test`)

| Layer | Count | Notes |
|---|---|---|
| Unit tests (`src/**`) | 105 | Policy table and invariants, classification, stores, parsers, schedules, workflows, usage, memory |
| Integration tests (`tests/integration.rs`) | 8 | Real sockets: model client against an SSE server, HTTP API, MCP client↔server, Telegram against a stand-in, usage/routing, interface script syntax, a real browser |
| Optional | 1 | Neural memory (`--features neural -- --ignored neural`) |

The real-browser test runs only when `HERON_TEST_BROWSER` is set (`1`, or a path). CI sets it on
all three systems.

## Security tests and what each proves

| Test | Proves |
|---|---|
| `permissions::tests::the_decision_table` | Every (effect, mode, taint) cell of the policy, written out |
| `permissions::tests::promises_that_hold_whatever_the_settings` | 600 combinations: forbidden never runs, deny beats allow, read-only changes nothing, tainted change/egress is never auto-allowed (including unrestricted), secrets always ask; and that plain reading is still allowed (not over-blocking) |
| `agent::tests::every_built_in_tool_declares_what_it_changes` | Every built-in tool has a classified effect; none that changes state is treated as a read; read-only refuses each |
| `mcp::tests::another_app_cannot_change_anything_in_read_only_mode` | The MCP server entry point uses the same policy and records refusals |
| `agent::tests::after_outside_content_nothing_outbound_or_lasting_happens_unasked` | A page cannot cause a fetch of a built address, a scheduled job, or a planted memory; only the user's own addresses count |
| `app::tests` (approvals) | 20 simultaneous approvals carry the action out once; an interrupted one is "unknown" |
| `app::tests::a_due_job_is_claimed_once_recorded_and_never_silently_lost` | Two schedulers cannot both take a due job; a run cut short by a restart is reported, not repeated |
| `tools::fs::undo_tests` | Undo never overwrites a file edited since; failed writes leave nothing behind |
| `tools::shell::tests::commands_see_a_short_list_of_variables_…` | Tokens and keys in the environment are not visible to commands |
| `tools::shell::tests::sandboxed_commands_…` | Real containment on Linux and macOS (skipped where no sandbox exists) |

## What is missing

- Fuzzing of the parsers (model stream, URL checks, paths, shell classification, MCP framing,
  workflow templates, ICS, mail).
- Mutation testing of `permissions`, approvals and execution.
- Concurrency tests beyond approvals; crash/restart fault injection for jobs.
- Fixtures for malicious email, MCP server and skill content.
- Accessibility and responsive checks as automated tests (done by hand with screenshots so far).
- Live provider conformance (only two free OpenRouter models have been run by hand).

## Rules

- A test is not changed to make a failure go away; if behaviour changes on purpose, the test and
  the decision record change together.
- Network is not used by tests except against servers the test starts itself.
- Real-model runs are recorded in the changelog as manual checks, never as automated passes.
