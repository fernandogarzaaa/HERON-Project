# 08. Reliability and recovery

**Result: FAIL.** Several invariants have automated tests that passed in this run's local
suite (285 tests on the combined tree, report 03). Process-kill, disk-full, real upgrade and
restore-from-backup drills were not run. Recovery times were not measured.

"Covered" below means a named test exercises the scenario and passed at `97a418e4` (combined
tree) on macOS. It does not mean the scenario was injected into a running app.

| # | Scenario | Coverage | Evidence |
|---|---|---|---|
| 1 | Model provider unavailable | Covered: retries then falls back; auth failures stop at once with guidance | `integration::http_client_streams_retries_and_falls_back`, `auth_failures_stop_immediately_with_guidance` |
| 2 | Network disconnect | Partly: same retry path; Telegram poll errors are swallowed and retried (PR #35 C35-5 notes a spin risk) | as above; review-pr35 |
| 3 | Process termination | Covered for the connector registry lock (killed holder releases it) and for interrupted runs (marked `unknown`, not repeated) | `connectors::tests::terminated_process_releases_registry_lock_without_stale_lock_cleanup`, `app::tests` |
| 4 | Forced restart | Covered: approvals and due jobs claimed atomically; interrupted ones reported, not replayed; Telegram update offset persisted (PR #35) | `a_due_job_is_claimed_once_recorded_and_never_silently_lost`, review-pr35 |
| 5 | SQLite busy/locked | Partly: 12 simultaneous starts tested; PR #35's spool shows `database is locked` under two writers (C35-2) | `store::upgrade_tests`, review-pr35 |
| 6 | Corrupted configuration / secrets | Covered: damaged secrets file never overwritten; damaged database refused, not upgraded; damaged connector registry not ignored | `a_damaged_secrets_file_is_never_overwritten`, `store::upgrade_tests`, `connectors::tests::damaged_registry_is_not_ignored_or_overwritten` |
| 7 | Expired credentials | Partly: vault write rollback tested; OAuth expiry (PR #26) has no recovery path except deleting data (O26-2) | `credential_transaction::tests`, review-pr26 |
| 8 | Token rotation | PR #35: a wrong token silently unpairs (C35-1); PR #26: rotation via verified vault transaction | review-pr35, review-pr26 |
| 9 | Interrupted task execution | Covered: interrupted approvals become `unknown` (D-6) | `app::tests` |
| 10 | Duplicate remote message | Covered in PR #35: dedup by (kind, sha256(token), update_id) | `channel::` tests, review-pr35 |
| 11 | Slow tool response | Partly: SSE back-pressure bounds; command output ceiling | `stalled_chat_reader_cannot_accumulate_unlimited_events`, `endless_output_is_cut_off_instead_of_filling_memory` |
| 12 | Cancelled tool execution | Covered: `/stop` on the phone halts the reply; desktop cancellation in chat tests | Telegram integration test, `ui::chat::tests::event_flow_error_and_cancel` |
| 13 | Disk full / write failure | Partly: a failed write leaves nothing to undo and no stray files | `a_write_that_fails_leaves_nothing_to_undo_and_no_stray_files` |
| 14 | Upgrade and data migration | Covered for the database format from 0.1.0 with data in every table; not run against a real 0.3.x install | `store::upgrade_tests` |
| 15 | Restore from backup | Covered at unit level, including restore over a damaged database | `store::upgrade_tests` |

## Invariants

| Invariant | Status |
|---|---|
| No unauthorized action after restart | Holds for approvals and jobs (tests above). |
| No duplicate destructive tool execution | Holds for approvals (20 concurrent → 1, `app::tests::scheduled_job_runs_once_readonly_and_lands_in_inbox`). PR #35 C35-2 shows a stale lease can resend a reply; replies are not tool actions. |
| No credential resurrection after revocation | PR #26 O26-4: a failed disconnect strands data with the credential gone (traced). |
| No cross-account leakage | PR #26 O26-5: merged provenance makes per-account erasure delete a user-confirmed fact and hide a legacy one (probe-confirmed by the reviewer). |
| No silent loss of committed state | PR #35 C35-3: retry exhaustion drops a reply silently. |
| No unbounded retry storm | PR #35 C35-5: persistent spool error spins the poll loop. |
| No duplicate background workers | Holds on `main` (OS lock on `background.lock`); PR #35 Telegram gateway has no single-instance guard (C35-2). |

## Not run

Kill -9 during a database upgrade, disk-full injection, upgrade from an installed 0.3.1 app
with real data, rollback to 0.3.1 after an upgrade, and time-to-recovery measurement. Each needs
a disposable user profile and the packaged app.
