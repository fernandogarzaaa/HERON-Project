# Stage 8: intermediate zoom-out review (resolved)

Status: resolved

**Target:** `docs/designs/heron-multi-account.md` (full file, draft `1fe263b`).
**Reviewer:** `egm-zoom-out` (Goldfish). It was told to focus on what stages 6 and 7 added.
**Result:** 0 Blockers, 4 Majors, 3 Nits. All are fixed. For #4, the reviewer's default is adopted and the alternative is left for the owner.

| # | Sev | Finding | Decision | Change |
|---|---|---|---|---|
| 1 | Major | There were three overlapping ways to erase data from a restored backup. Remove failed when two or more accounts were listed, and the fold could file data under the wrong account. | FIX | Dropped the fold, `old_items`, the Settings row and `removeOldData`. `disconnect_source` now refuses bare OAuth names only while the old token exists, so one route erases bare rows. The README says the backup is for downgrades only. |
| 2 | Major | The migration hook was named at `server::serve`, which nothing calls. The Mac app starts through `start_endpoint` → `server.rs:261`, so the owner's data would never migrate. | FIX | The migration now runs inside `App::own_background_work()` the first time it gets the lock (`app.rs:2112-2114`). That covers `start_endpoint`, the daemon and a Heron taking over. §8 states the cost on takeover (serialised by `SOURCE_WRITES`) and the lock order. |
| 3 | Major | Blocking sync while the upgrade was incomplete guarded a state that a different step order removes. The order also broke the registry-first rule. | FIX | Steps reordered: rename, registry, record, commit. The sync and learn gate is removed. The cleanup when a migrated account is disconnected stays (I-5). |
| 4 | Major (owner's call) | A server-side Cancel plus a client-visible `exchanging` state was a lot of machinery for a sign-in the user approved at the provider. | FIX (default), Q-4 | Cancel is local only, as today. `exchanging` is internal, and polls report `waiting`. The cancel route, the wire state, the Swift method and two tests are removed. Q-4 offers the server-side cancel as the owner's alternative. |
| 5 | Nit | The Add client-ID order changed from today's. | FIX | Today's order is kept: body, then stored (most recent listed record), then bundled. |
| 6 | Nit | The hash-collision row came back. | FIX | IDs are now 128 bits (32 hex). The collision row, the `subject` record field and the `admit` parameter are dropped. Test vectors are recomputed. |
| 7 | Nit | Checks listed the tests once per test. | FIX | U1 and U2 capture the list once and grep that. All four checks pass `bash -n`. |
