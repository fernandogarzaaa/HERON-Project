# Stage 5, Initial Zoom Out: heron-computer-use.md

Reviewer: `egm-zoom-out` (Goldfish, fresh context). Target read at the first draft (commit 63c62f1). Verdict: revise (1 Blocker, 7 Major, 12 Nit).

| id | severity | finding | decision |
|---|---|---|---|
| 1 | Blocker | Driving Mail and Messages reaches the Refused stores `Library/Mail` and `Library/Messages` (`src/permissions.rs:889-895`) and sends mail, against heron-v1-features.md's "never sends" non-goal | Fixed. Mail and Messages are hard-denied in Phase 1; CQ7 asks the owner whether to lift that, with its own accepted risk; CUJs moved to Notes and Reminders |
| 2 | Major | The permit went through `WebApprover` plus a second route: Yolo skips `Privileged` asks, approvals park after 900 s, CUJ 7 contradicted the route | Fixed. `computer_start` asks the app over the bridge; the app shows the sheet and answers with the permit; no `/lease` route, no parking (Alternative F records why) |
| 3 | Major | Step approvals were answerable by any token holder and parked | Fixed. The driver shows step cards itself and acts only on its own **Allow once**; all `computer_act` calls are `ExternalWrite` in the core |
| 4 | Major | PR #84's **Run again** replays the call; no `Resumer`; wrong `RepeatGuard` signature; start/stop would be journaled | Fixed. The run is one action entry with a `computer` resumer; CQ11 asks PR #84 to fold tool entries into it; the app drops permits on core restart; start and stop are `LocalRead`; signature corrected; no extra hooks |
| 5 | Major | The lease was stored in four places | Fixed. `computer_leases` table dropped; the app's `PermitBook` is the authority; the core keeps a cache |
| 6 | Major | "Reuses" pointed at unbuilt Harness2 and B2 pieces | Fixed. Listed as "designed, not built"; the `Desktop` seam and `AppPolicy` are placed for Harness2's observers to reuse; the B2 `FeatureKey` migration is stated; drive cards reuse the suggestion's own session |
| 7 | Major | `drag`, `double_click` and the Finder folder fence | Fixed. Both actions cut; Finder hard-denied in Phase 1 |
| 8 | Major | `computer_stop` would ask in every tainted run | Fixed. `LocalRead` |
| Nits | Nit | Anthropic action-name claim; `read_file` not always `LocalRead`; `delegate` during a lease; CQ7 skip-learn redundant; Alternative B TCC claim; weak C2 check; C3 order; client file names; duplicated fixtures; `## R-n` format and signature; "lease" name collision; browser tools also ask per click; scattered numbers | Fixed: sentence dropped; the lock offers only the five computer tools and refuses `delegate`; skip-learn removed (I4 row cites `src/agent.rs:825-831`); Alternative B softened; C2 greps `pub fn guarded` and `pub fn register_resumer`; pre-step says C3 waits; files named; one fixture copy; `## R-2` awaiting signature; renamed "permit"; Non-Goals note the browser tools; a Draft parameters table |
