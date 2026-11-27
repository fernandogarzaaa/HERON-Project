# Stage 5 Zoom-Out Re-audit: heron-v1.md

**Target:** `docs/designs/heron-v1.md` at ae027d1.
**Reviewer:** `egm-zoom-out`, read only. This was the one re-audit the stage allows.
**Previous findings:** 16 in total. The first report's header said 17, which was a miscount. 15 are resolved and 1 (#3) is partly resolved; its remainder is N2 below.

## New findings and how they were handled

All five were fixed in the doc in the commit after ae027d1. They were not re-audited, because the re-audit limit is reached. Stages 6 to 9 review the whole doc again.

| # | Severity | Finding | Fix applied |
|---|---|---|---|
| N1 | Blocker (missed by the first pass) | The new read tiers dropped existing secret protections. Workspace `.env`, `*.pem` and `credentials.json` became free to read, `~/.ssh` and `~/.aws` dropped to a plain "Ask", allow rules could open them, and `sources.rs` and `ambient.rs` would index secrets. | `sensitive_path()` stays as the first check in every tier, including the workspace. It still asks in every mode, before allow rules, and its pattern is extended with the M-4 paths. Read tiers apply only to paths it doesn't match. The check order is: secret, Refused, allow rules, tiers. `sources.rs` and `ambient.rs` are in U1 scope. CUJ 3 is aligned. New tests: `workspace_env_and_key_files_still_ask`, `indexing_and_observation_skip_secrets_and_refused_paths`. |
| N2 | Major | "Parked, not denied" contradicted parking, which today means deny and run later. The existing 900-second live wait was ignored. | `WebApprover` publishes on the bus and waits on its existing answer map, with the 900-second timeout. Only the 15-second push to a single stream goes away. On timeout it falls back to today's parking. The test is renamed `api_approval_waits_on_the_bus_then_parks_on_timeout`. |
| N3 | Nit | The core's entry point was unstated, and the flag sat in the wrong layer. | The app launches `heron-cli serve --no-open --port 0 --print-endpoint`. The flag lives on `Cmd::Serve` and forces a bind to `127.0.0.1`. The item moved to `main.rs` and `server.rs`. |
| N4 | Nit | Ownership contradictions: which unit lands the sandbox profile, which merges the spike winner, whether Slint stays on macOS, whether there are two core binaries, and what `/chat` returns. | The sandbox profile lands in U2, with a build/git check. U2s only measures, and U2w merges. v1 removes Slint from the macOS bundle. The core is the existing `heron-cli`, with no Helpers copy, and the `cmp -s` guard still applies. The `/chat` first `session` event gains `run_id`. |
| N5 | Nit | `fs_reads_untrusted` would ship as a permanent config switch only to run a measurement. | It is built only under the `spike` cargo feature. U2w either makes it fixed behaviour or deletes it. |
