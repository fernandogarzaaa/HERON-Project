# Qualification report

Written 13 November 2026, after the first working session on the engineering mission. This is a
progress report, not a completion report: most of the mission is not done. Section 2 says
exactly what is.

## 1. Commits and pull requests

| Thing | Value |
|---|---|
| Audited `main` | `8ecd2fa3e78f36a734eadda913ed68244002b242` |
| PR #1 (v0.1.0 beta) | Merged into `main` |
| PR #2 `heron-v0.1-beta` | Merged into `main` by the owner on 13 November 2026 (merge commit `5b1bdf2`), at head `9293554` with CI green and the review findings addressed |
| PR #4 `security/effect-model` | Merged into `main` by the owner on 13 November 2026. Holds the permission redesign and the owner's approval of D-2 and D-3 |
| PR #5 `durability/undo-jobs-env` | Open, marked ready for review by the owner. Undo conflict detection, atomic job claims, environment allowlist for commands, and the three findings from its automated review. Its base is still the already-merged `security/effect-model` branch and needs changing to `main` |
| PR #3 `feat/cli-providers` | Open, not a draft, opened by the repository owner outside this session, targets `main`. Head `f189aaa`. Its CI fails on the Windows and macOS test jobs. It changes `config.rs`, `llm.rs`, `main.rs`, `server.rs`, `assets/index.html` and the README, all of which PR #2 and PR #4 also change, so it will need reconciling with them. Not touched by this session |

PR #1 was merged by this session at the owner's request; PR #2 and PR #4 were merged by the owner. Nothing was force-pushed, and no release was created by this session (the owner published v0.1.0).

## 2. Remediation matrix

The full matrix is REMEDIATION_MATRIX.md. Summary by status:

- **Verified:** A1 effect model; A2 outside-content rule in unrestricted mode; A2 addresses from
  tool output are not the user's; A3 search exemption removed and narrow research grant;
  C1 atomic approval claim; MCP server entry point under the policy; B2 allowlisted environment
  for commands; C2 undo conflict detection and atomic writes (second session, branch
  `durability/undo-jobs-env`, stacked on PR #4).
- **Partial:** A1 across entry points (the app's own buttons are outside the mode, by decision
  D-3); A2 provenance; A3 typed egress policy; A4 invariant tests (no fuzz harness);
  B2 limits; B3 MCP; B5 atomic writes; C3 job claims and interrupted-run reporting; C4; D3; D4; E2; F1; G7.
- **Not started:** B1 (browser request interception, per-session state, screenshots);
  B2 Windows containment; B4; C5 migrations; D1–D2 capability router; E1, E3, E4; F2 Automation Studio; F3 OAuth; F4 voice;
  F5; F6; G1 gates; G2 mutation testing; G3 fuzzing; G5.
- **Blocked externally:** G4 signing and installer qualification; G6 branch protection; live
  OAuth tests; paid-model tests; removing the stray file from the v0.1.0 release.

## 3. Security evidence

- **Policy matrix:** `permissions::tests::the_decision_table` (10 effects × 4 modes × clean/tainted).
- **Invariants:** `promises_that_hold_whatever_the_settings`, 600 combinations, including
  permitted cases so over-blocking would also fail.
- **Tool contract:** `every_built_in_tool_declares_what_it_changes` (30 rows covering every
  built-in tool).
- **Attack tests that pass:** a page cannot cause a fetch of a model-built address, a scheduled
  job or a planted memory; another app cannot change state in read-only mode; twenty
  simultaneous approvals execute once.
- **Fuzzing and mutation testing:** none run. No kill-rate exists.
- **Unresolved, known:** T2–T10 in SECURITY_THREAT_MODEL.md. The most serious are the open
  network inside the command sandbox and no sandbox on Windows (T3), per-request checks missing
  in the browser (T4), shared browser state across conversations (T5), and recalled unconfirmed
  facts not tainting a conversation (T2).
- **Risk acceptances:** DECISIONS D-2 and D-3, approved by the owner on 13 November 2026 (recorded in DECISIONS.md).

## 4. Correctness and performance evidence

- Tests: 110 unit and 8 integration pass locally (Linux 6.18 x86_64, rustc 1.97.0), including
  the real-browser test against Chromium. Windows type-check passes from Linux.
- Hosted CI: green on PR #2's head. For the permission branch see its pull request; a local
  pass is not a hosted pass.
- Memory: the original 34-question figures are unchanged (28/34 and 30/34 built in; 31/34 and
  33/34 neural). No expanded evaluation, no 1K/10K/100K benchmark.
- Providers: two free OpenRouter models run by hand on a handful of tasks. No conformance suite,
  no latency or cost comparison.
- Crash/restart: only for approvals. Not for jobs, undo or the database.

## 5. Product delivery

Delivered and tested before this mission (PR #2): Usage page, model routing, interface pass,
conversation-wide grants, written comparison with five other agents. Not delivered: Trust
centre page, Automation Studio, explainable memory, OAuth connectors, voice, screenshots.

## 6. CI and release

- v0.1.0 is published as a pre-release with unsigned installers; only the Linux one has been run.
- No formatting, lint, advisory, mutation or fuzz gate exists (measured baseline in QUALIFICATION.md).
- Human actions needed: delete `index.html` from the v0.1.0 release; set up branch protection;
  provide signing identities; review and merge (or reject) PR #2 and the stacked permission PR.

## 7. Operating it

- Build, test and run: QUALIFICATION.md and the README.
- Roll back the permission change: revert commit `728184a`; no data migration is involved (the
  only schema use is the existing `approvals.status` column, with two new values, `running`
  and `unknown`).
- Behaviour changes users will notice are listed at the top of the changelog. The largest:
  read-only mode now refuses memories, tasks and reminders, and research tasks prompt more.

## 8. Verdict

**NOT READY** for production release.

The software remains **BETA / NOT PRODUCTION QUALIFIED**. It is not yet "ready for qualified
beta" as the mission defines it either: the gates in QUALIFICATION.md (remaining section A and B
items, undo conflicts, a durable job queue, lint and advisory gates, signed installers run on
real machines, owner sign-off) are open.
