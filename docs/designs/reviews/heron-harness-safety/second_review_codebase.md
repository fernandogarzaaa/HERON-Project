# Second review: codebase details (Goldfish, 2026-11-23)

Reviewer: egm-codebase-details. It saw only the design doc, the code in `src/`, and the local `fix/*` checkouts for #76 to #79. Scope: the mechanisms the first resolution added.

**Counts:** P0 0, P1 2, P2 5, P3 8.

## Evidence limits

- `git show` was unavailable to the reviewer, so it read the `.claude/worktrees/ultrafix-*` checkouts. #75 and `heron-harness2.md` weren't reviewed.

## P1

- **P1-1. A conversation grant bypasses OQ9.**
  - Code: `execute_call` turns a granted `Ask` into "approved-for-conversation" (`src/agent.rs:645`). Grants persist in kv `grants:<session>` (`src/agent.rs:131`), and `shell:git *` is grantable for `ProcessExec`.
  - Failure: after a restart, "continue" re-runs `git commit` silently.
  - Fix: a forced Ask skips the grant arm. Add a test with a persisted grant.
- **P1-2. Interrupted calls from different sessions, workspaces or taint merge into one approval.**
  - Code: the merge key in `src/store.rs:1062` is `status='new' AND tool AND args AND origin`, with no session.
  - Failure: two sessions with an interrupted `cargo test` share one approval, which carries the first row's taint and workspace.
  - Fix: make the origin unique per session (`interrupted: <origin> <session_id>`), and define `close_asked` for several rows.

## P2

- **P2-1. The `repair_transcript` text claims an approval exists for every dangling call.**
  - Code: `src/agent.rs:274-298`, called at `:475`. Cancelled and read-only calls get no approval.
  - Fix: use neutral wording.
- **P2-2. OQ9 says a job's approver is `DenyAll`.**
  - Code: jobs use `ParkingApprover` (`src/app.rs:1553`, `src/permissions.rs:526-531`). A repeat would park a second approval.
  - Fix: in a job, the OQ9 deny must not park.
- **P2-3. `Tool::protected` and `Call::refusal` touch far more code than file 9 lists.**
  - Code: 30 `FnTool` literals, 17 `Call` literals, and an exhaustive destructure at `src/permissions.rs:365`.
  - Fix: list the files, add a constructor, or derive the label inside `execute_call`.
- **P2-4. The Pre-S1 skeleton doesn't compile.**
  - Issue: an empty `journal/mod.rs` can't resolve `journal::sql::step`.
  - Fix: declare `pub mod sql; mod lease;`.
- **P2-5. `run_job` and `run_automation` don't receive `job_runs.id`.**
  - Code: the id is created at `src/app.rs:1498`.
  - Fix: open the frame in `run_due_schedules`, in "Run now" (`:1703`), and in the trigger caller (`:1755`).

## P3

- **P3-1.** Refused is not in `mutates()` (`src/permissions.rs:89-100`). Fix the sentence that says it is.
- **P3-2.** A `.git` path-component refusal has no label. Add "a git repository's internals".
- **P3-3.** `origin` has no source in `ToolCtx`. Use `Store::session_origin` (`src/store.rs:794`).
- **P3-4.** The turn run id duplicates the server's run id (`src/server.rs:550`). Pass the existing one.
- **P3-5.** `RunFrame` needs the `EventSink`. The drop guard needs `Handle::try_current`.
- **P3-6.** `eval/require-tests.sh`: exit code 2 on misuse is undocumented, it discards cargo's stderr, and it matches only the last path component. Also, `verify` doesn't run it.
- **P3-7.** The `resume:` branch orders its checks differently from today's `run_parked`. Say it is a new branch.
- **P3-8.** The dismiss-as-claim change affects every approval (`src/server.rs:1183-1186`). Say so, and add a test.

## Verified accurate

The doc's line references in `agent.rs`, `store.rs`, `app.rs` and `server.rs` hold, as do its claims about #76 to #78. `with_conn` fits a Pass 1 transaction, and the baseline key names follow the existing convention. A tool named `resume:…` can't collide with MCP tool names.
