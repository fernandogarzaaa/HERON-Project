Status: resolved

# Second review: resolution (2026-11-23)

**Target:** `docs/designs/heron-harness-safety.md` (full file)

**Inputs:**

*   `second_review_design.md`. Its ids are prefixed `D-`. The two uncounted items next to the scope are `D-Adj1` and `D-Adj2`.
*   `second_review_codebase.md`, prefixed `C-`.
*   `second_review_readiness.md`, prefixed `R-`.

**Totals:** 59 items (18 design, 15 codebase, 26 readiness): **FIX 57, CONTEST 2, DISMISS 0.** C-P3-6 counts as FIX, though its line also contests part of the finding.

**Merged findings,** each fixed once:

| Issue | Fixed under | Also covers |
|---|---|---|
| OQ9 gate bypassed by a conversation grant | D-F2 | C-P1-1, R-P1-4 |
| Approval merge ignores session and taint | D-F4 | C-P1-2, R-P2-2 |
| `repair_transcript` text | D-F13 | C-P2-1 |
| `guarded` failure policy | D-F7 | R-P1-2 |
| RunFrame shape | D-F12 | R-P1-3, C-P2-5, C-P3-4, C-P3-5 |
| `Call::refusal` | D-F11 | R-P1-5, C-P2-3 |
| `reopen_approval` returning false | D-F6 | R-P2-1 |
| Workspace check | D-F15 | R-P2-5, with D-F8 |
| Baseline export and import | D-F16 | R-P2-7 |

**Owner holds:**

*   OD1 to OD8 are byte-identical. Their SHA-1 matched before and after the edits.
*   No OQ default or recommendation changed.
*   OQ9's job clause wrongly named `DenyAll`. It now names `ParkingApprover`, and one sentence on the trade-off was added.
*   OQ1 gained one sentence about the W5 hold, and OQ11 one sentence: its alternative would need its own insert path, since the ask pass and the gate are keyed on an `effects` row.
*   The four invariants still hold. §4 I2 gained two lines, for the per-session merge and for grants under the gate.

**Found while fixing:** the W7 check required 14 threat-model entries, but the Risks table had 13 rows, so the check could never pass. The table now has 15 rows, and `check-w7.sh` requires at least 15.

## Design (`D-`)

| id | Decision | One line |
|---|---|---|
| D-F1 | FIX | §1.3: the drop guard checks for a panic, then for a Stop flag that `/api/cancel` and Telegram's `halt` set before they abort. Every other drop writes `interrupted`, so a quit or crash of the Mac app asks again; a failed Drop write leaves `intent`, which recovers as interrupted. The frame clears the Stop flag only after its inner future has dropped. The app's quit closes stdin and calls no cancel (`HeronApp.swift:42-45`), so a quit lands on `interrupted`. |
| D-F2 | FIX | New §1.6: the gate runs after `decide`, forces an ask that skips the `session_grant` arm, and treats Always and Session answers as Once. Tests: `persisted_grant_does_not_bypass_the_ask_again_gate`, `always_answer_to_a_gated_call_adds_no_rule_or_grant`. |
| D-F3 | FIX | `pending_ask_again` also matches `interrupted` rows and dead-lease `intent` rows. Recovery stays with the background owner, because two recovering processes could both insert an approval (stated in §1.4). |
| D-F4 | FIX | The ask pass inserts its own approval in one transaction with the row. It merges only rows with the same session, tool, arguments hash, taint and folder, and never uses `park_approval`'s merge. G1 and §4 I2 are updated. |
| D-F5 | FIX | New `child_pgid` column. Recovery waits up to one hour while the process group is alive, and a dropped shell call kills its group. One Risks row added. |
| D-F6 | FIX | Reopening accepts `running` or `unknown`, and falls back to a new approval. `recover_interrupted_approvals(skip)` leaves live parked calls alone. |
| D-F7 | FIX | §1.3 fails closed: `Guarded::NotRun`, an audit row `journal-failed` and an `Error:` result. A failed outcome write leaves `intent`. The timeout text matches `src/agent.rs:687`. |
| D-F8 | FIX | `answered` is removed. A blocked Run again on an ask-again goes back to `new` (`requeue_approval`), and a failure that ran is final. The workspace check doesn't apply to `resume:`. |
| D-F9 | FIX | Approving the gated call claims the waiting ask-again first (`supersede`). `Gate::Running` matches an approval in `running` and denies. |
| D-F10 | FIX | `is_dead` opens without creating. A clean exit removes the process's own lease files. Recovery sweeps lockable lease files while holding their locks. |
| D-F11 | FIX | `protect::Refusal { path, label }` comes from one `refusal_for`. The text is built after `decide` by `refused_text`, in `execute_call` and `run_parked`. `.git` paths get the label "a git repository's internals". |
| D-F12 | FIX | `RunFrame` is a struct sketch, with nesting rules and an end-of-run table: finished, Stop, panic, shutdown. It records `no-run-frame` outside tests, snapshots on `spawn_blocking`, and records a rehash skipped at shutdown in the audit log and inbox. |
| D-F13 | FIX | `repair_transcript` is a pure function and can't see the journal, so one neutral text covers cancelled, read-only and interrupted calls. |
| D-F14 | FIX | The `rerun` and `dismissed` states are dropped, and `asked` is the last state. Prune joins on the approval's status. |
| D-F15 | FIX | The check compares canonical paths, shows the folder in the detail, and has each process compare against its own folder. It runs after the claim and before the taint and the policy. |
| D-F16 | FIX | §3.4 adds the baseline schema with `format: 1` and relative paths, the same `HOME` in the child, and fail-closed export and import. Import refuses an existing baseline and is listed as the third writer. |
| D-Adj1 | FIX | The stat refresh stores the stat taken before hashing, only when a second stat taken after hashing equals it. |
| D-Adj2 | FIX | `trust_current` takes the `since` the owner saw, and a stale one gets 409. The Mac call passes it. |

## Codebase (`C-`)

| id | Decision | One line |
|---|---|---|
| C-P1-1 | FIX | Merged into D-F2. |
| C-P1-2 | FIX | Merged into D-F4. `close_asked` is gone, and rows that share an approval are keyed by `approval_id`. |
| C-P2-1 | FIX | Merged into D-F13. |
| C-P2-2 | FIX | `Approver::live()` has no default, and every impl states its value: `ParkingApprover`, `DenyAll` and `AllowAll` return false, and `TerminalApprover` returns true only on a terminal. The gate then denies without parking. OQ9's factual clause is corrected. Test: `gated_call_in_a_job_is_denied_without_parking`. |
| C-P2-3 | FIX | Merged into D-F11. `Call`, `decide`, `Tool` and the 30 `FnTool` literals are unchanged. `src/tools/fs.rs` is listed, and MCP keeps the generic text because it serves no write tool. |
| C-P2-4 | FIX | Pre-S1 declares `pub mod sql; mod lease;`, `pub mod proxy;` and `pub mod scan;`. |
| C-P2-5 | FIX | Merged into D-F12. `run_job` gains `run: Option<i64>` from `run_due_schedules`, and `run_now` and `run_triggers` pass `None`. |
| C-P3-1 | FIX | Terms and §1.1 now say that Refused is not in `mutates()`. |
| C-P3-2 | FIX | The label "a git repository's internals" is added. |
| C-P3-3 | FIX | `execute_call` reads the origin with `Store::session_origin`, and `run_parked` uses the approval's origin. |
| C-P3-4 | FIX | Merged into D-F12. `turn_run_id(run_id)` uses the caller's run id, which is also the Stop flag's key. |
| C-P3-5 | FIX | Merged into D-F12. `RunFrame` holds the `EventSink`, and the drop path uses `Handle::try_current()`. |
| C-P3-6 | FIX | Pre-S1 makes `require-tests.sh` print cargo's stderr. Exit 2 and the last-component match are already documented in its header (`eval/require-tests.sh:6-8`), and verify's full suite needs no name guard. |
| C-P3-7 | FIX | §1.5 says the `resume:` path is a new branch, ordered differently from the tool path. |
| C-P3-8 | FIX | §1.5 says the dismiss claim applies to every approval. Test: `dismiss_of_a_running_approval_is_refused`. |

## Readiness (`R-`)

| id | Decision | One line |
|---|---|---|
| R-P1-1 | FIX | Pre-S1 copies the five anchor files into `ref/anchor-harness/`, deleted before the PR. §3.2 lists the `default_deny_sockets` paths. |
| R-P1-2 | FIX | Merged into D-F7. |
| R-P1-3 | FIX | Merged into D-F12. |
| R-P1-4 | FIX | Merged into D-F2. §1.6 also gives the ask text, and says the waiting approval is retired when the gated call is approved. |
| R-P1-5 | FIX | Merged into D-F11. `user_home` and `heron_homes` are named. |
| R-P1-6 | FIX | §1.2 has the exact `CREATE TABLE` and the three `CREATE INDEX` statements, and Pre-S1 runs exactly that DDL. |
| R-P2-1 | FIX | Merged into D-F6. |
| R-P2-2 | FIX | Merged into D-F4. Several rows per approval is now stated, and `workspace_for` relies on the merge key. |
| R-P2-3 | FIX | `run_parked` guards only effects that `mutates()`. Test: `parked_read_is_not_journaled`. |
| R-P2-4 | FIX | A `resume:` rerun row has kind `action`, the handler, the subject and the original label, and the `asked` row is left as it is. |
| R-P2-5 | FIX | Merged into D-F15 and D-F8. |
| R-P2-6 | FIX | §3.3: the first allowed mutating call takes the snapshot, after the verdict and before the tool, on `spawn_blocking`. Denied and waiting calls don't. |
| R-P2-7 | FIX | Merged into D-F16. |
| R-P2-8 | FIX | File 3 names the five tests in `egress::proxy::net`. |
| R-P2-9 | FIX | The W3 test is now `rules_deny_the_resolver_socket`. The ordering check stays with W6b's `egress_rules_come_after_local_socket_rules`. |
| R-P2-10 | FIX | Pre-S3 has a manual hold, because skein has no hold state (checked with `skein node add --help`). W5 and later nodes are added after OQ1 is answered. |
| R-P2-11 | FIX | Alternative W6a, W6b and W8 rows are given for OQ1's stronger option. `check-w8.sh` takes `default` or `stronger`, and W7 and verify work under either answer. |
| R-P2-12 | FIX | W6b runs `check.sh w6a w6a-toolchain w6b`, which needs W6a's names too and runs the whole lib suite. |
| R-P2-13 | FIX | `check.sh` sets `HERON_REQUIRE_SEATBELT=1` (`src/sandbox.rs:522-526`), so a Seatbelt test can't pass by skipping. Run notes name verify's testteeth step as the gate on what the tests assert. |
| R-P2-14 | FIX | Pre-S3 sets `--max-retries 1`, at most three attempts per unit, and a stop rule (no new run, report, ask the owner, never weaken a check). It defines the "Supervisor" label, and W4 is marked Supervisor because it uses `sandbox-exec`. |
| R-P2-15 | FIX | The W6a, W6b, W7 and W8 checks are scripts in `eval/harness-safety/`, written out in Pre-S1. The `\|` note is removed. |
| R-P3-1 | FIX | The header says line numbers are at `39dbdf5` and that function names are the stable reference. Nearly every reference already names one. |
| R-P3-2 | FIX | `recover` returns the number of approvals it created or reopened. |
| R-P3-3 | CONTEST | `Store::db` already recovers a poisoned lock (`self.conn.lock().unwrap_or_else(\|e\| e.into_inner())`, `src/store.rs:778-779`). `with_conn` uses it, and the doc now says so. |
| R-P3-4 | FIX | `ToolchainState` and `EgressRules` derive `Debug` and `Clone`, like the protect types. |
| R-P3-5 | CONTEST | A broader substring filter only runs more tests, never fewer. The existence guard matches exact last components (`eval/require-tests.sh:37-43`), and no module path outside each new module contains `journal::`, `repeat::`, `egress::`, `protect::` or `toolchain::` today. |

## Left for the owner

*   OQ1, OQ3, OQ7, OQ8, OQ9, OQ10, OQ11 and OQ12 are unchanged and still open.
*   Pre-S3 still needs the owner's approval to run `skein run` outside Claude Code's sandbox.
