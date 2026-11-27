# Second review: design (Goldfish, 2026-11-23)

Reviewer: egm-design, using the persona `agents/design_critic.md`. It saw only the design doc and the code at this worktree's HEAD, which doesn't include #75 to #79. Scope: the mechanisms the first resolution added.

**Counts:** P0 0, P1 4, P2 5, P3 7.

## P1

- **F1. `guarded`'s drop guard writes `cancelled` on any drop.**
  - Issue: a clean runtime shutdown also drops the guard. The core exits when its stdin closes, which happens when the Mac app quits or crashes (`src/main.rs:829-833`).
  - Failure:
    1. The Mac app quits during `git commit`.
    2. The row is marked `cancelled`, so no ask-again is created.
    3. The repaired transcript claims an approval exists.
    4. "continue" re-runs the commit silently.
  - Fix: `/api/cancel` and the Telegram stop set a cancel flag before they abort. The guard writes `cancelled` only when that flag is set, and `interrupted` on any other drop.
- **F2. A saved conversation grant bypasses the OQ9 gate.** `src/agent.rs:645`, `:131`, `:169`.
  - Fix: a gated call skips the `session_grant` shortcut and records no new grant from that answer.
- **F3. `pending_ask_again` sees only `asked` rows.**
  - Issue: a crashed call stays `intent` until the background owner recovers it, and `heron chat` and `heron run` never recover.
  - Fix: also match `intent` rows with a dead lease, `interrupted` rows, and approvals that are `running`. Optionally, recover at CLI startup.
- **F4. The `park_approval` merge ignores session and taint.** `src/store.rs:1062`.
  - Issue: a tainted session's interrupted call can run under an untainted session's taint and grants, which conflicts with I2.
  - Fix: put the session in the origin, or give ask-again its own insert. One approval per session.

## P2

- **F5. The interrupted effect may still be running.**
  - Issue: the lease is released when Heron dies, but an orphaned child keeps going.
  - Fix: one lease per call, inherited by that call's child process tree, or record the child's process group and start time.
- **F6. The case where `reopen_approval` returns false is unspecified.**
  - Issue: a takeover can mark the approval `unknown`, and several rows can share one run.
  - Fix: `ask_again` reuses a `new` approval, reopens an `unknown` one, and otherwise parks a fresh one. Make `recover_interrupted_approvals` skip approvals with a live `intent` row.
- **F7. `guarded` doesn't say what happens when the lease or intent write fails.**
  - Fix: fail closed and don't run the tool. If the outcome write fails, the row stays `intent`.
- **F8. A blocked or failed Run again is final.**
  - Issue: `answered(Rerun)` runs before the checks, and a `resume:` entry can get stuck.
  - Fix: call `answered` only after the run. A recoverable block puts the approval back to `new`. On a final failure, `declined`. State that the workspace check doesn't apply to `resume:`.
- **F9. An approved OQ9 question leaves the ask-again waiting, and a Run again in progress isn't gated.**
  - Fix: retire the matching ask-again, and match `running` approvals.

## P3

- **F10.** Lease files: unlink your own on a clean exit, sweep orphans while holding the lock, and have `is_dead` open without creating.
- **F11.** `Call::refusal` needs the path. Make it `Option<(String, &'static str)>` from one shared `protect::refusal_for`, fill it in `run_parked`, and label `.git` refusals.
- **F12.** `RunFrame`:
  - A missing frame outside tests writes an audit row.
  - Take the snapshot on a blocking thread.
  - Say what happens to a rehash skipped at shutdown.
- **F13.** The `repair_transcript` text should follow the row's state, with a separate text for `cancelled`.
- **F14.** The `rerun` and `dismissed` states duplicate the approval's status. Prune by joining on the approval.
- **F15.** The workspace check: compare canonical paths, show the folder in the detail, and handle two processes working in different folders.
- **F16.** Baseline export and import:
  - fail closed;
  - add a format version;
  - use relative paths and the same home in the child;
  - `import_baseline` refuses when a baseline already exists, and it is listed as the third writer.

## Next to the scope (not counted)

- The stat refresh must record the stat taken before hashing, and check it is unchanged after.
- `trust_current` should take the `Changed` record the owner saw.
