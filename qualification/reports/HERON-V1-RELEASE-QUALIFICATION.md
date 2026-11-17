# HERON v1.0 release qualification

Run date: 2026-11-17/18 (UTC). Machine: MacBook Pro, Apple silicon, macOS 27.2, rustc 1.99.1.
Repository: `fernandogarzaaa/HERON-Project`. Qualification branch: `qualification/v1.0-rc1`.

## 1. Executive verdict

**NOT READY.**

Three independent things each block a v1.0 release:

1. **Security.** `main` has one confirmed P0 and six confirmed P1 findings at its trust
   boundaries, each with an executable reproduction. The P0: in the default Ask mode, text in a
   workspace file steered the agent to read a credential file and call `web_fetch` with the token
   in a model-built URL, with no approval asked (M-1).
2. **The two feature PRs cannot be merged.** #35 and #26 are drafts, and their live provider tests
   (Telegram bot, Google and Microsoft OAuth apps) need credentials this run did not have. Each also
   has open P1/P2 findings.
3. **Platforms and release engineering.** No Windows or Linux runtime evidence beyond hosted
   build-and-test, no installer/upgrade drill on any OS, no signing identity, no branch protection.

What did pass: on macOS every required CI step passes on `main` merged with each PR and with both
together, once the test-isolation fix (#39) is included. Hosted CI is green on 3 OSes for `main`
and both PR merge refs. The native credential vault round trip passes against the real Keychain.

| Gate | Result | Evidence | Blocker |
|---|---|---|---|
| Repository audit | PASS (completed) | reports 01, 02 | none for the audit itself; it found P0/P1s |
| PR #35 | FAIL | §4, `evidence/reviews/review-pr35.md` | draft; live Telegram test needs a bot; C35-8 P1 |
| PR #26 | FAIL | §5, `evidence/reviews/review-pr26.md` | draft; O26-1/O26-2 P1; OAuth apps needed |
| Local CI | PASS for required steps with #39 merged; FAIL on `main` alone (host-dependent tests) | report 03, `evidence/ci/` | #39 to merge |
| Security | FAIL | report 07, `evidence/repro/` | M-1 P0; 6 P1; owner decisions |
| Mutation testing | FAIL (gate not met): `permissions.rs` 69.1% on main, 87.9% with PR #41; other critical modules not run | report 04, `evidence/mutation/` | run the remaining modules; merge #41 |
| Genesis evaluations | NOT_RUN | report 05 | approved model budget |
| Slint UI | FAIL (no real-window evidence; M-UI-1 P2) | report 06 | a person or UI driver on a real display |
| Reliability | PARTIAL → FAIL for release | report 08 | drills not run; #35/#26 invariant breaks |
| macOS | PARTIAL | reports 03, 09 | installer/upgrade/signing not run |
| Windows | BLOCKED (hosted build/test PASS only) | report 09 | Windows machine; signing certificate |
| Linux | BLOCKED (hosted build/test PASS only) | report 09 | Linux machine |
| Release candidate | NOT_RUN | §14 | all of the above |

## 2. Repository and baseline SHAs

| Thing | SHA |
|---|---|
| `main` | `449231fcbef29726a18aafe5937fa197b9e80c0a` (tree `e7e659a088141b6909d2062b3d7982126d8d073e`) |
| PR #35 head | `1c348ab93323950e4a0d57fd2b8548ff75bb6f25` (draft; unchanged from the brief) |
| PR #26 head | `183f3da50137d41f07e17fd81940e1df761ddb04` (draft; unchanged from the brief) |
| #35 merged onto main | local `befed831…`, tree `bdd16e29…` = GitHub `refs/pull/35/merge` `6e80e185…` |
| #26 merged onto main | local `a971f747…`, tree `d28adef1…` = GitHub `refs/pull/26/merge` `1cc9dc7c…` |
| #35 + #26 onto main | local `4e60e290…` (no hosted run exists) |
| PR #39 (test isolation) | `18e798f5217ea51f5d123e761dbb6d7db02e4492` |
| PR #40 (read gate) | `448227d941d4e3ceb348e701a25ee4b811509244` |

Details: report 01.

## 3. Architecture audit findings

Report 02 holds the component map, the capability classification and the full register.
Counts on `main`: **P0 1, P1 6** (M-1; M-2, M-3, M-4, M-5, M-6, C35-8), plus repository
governance M-GOV-1 (P1). PR-specific P1: O26-1, O26-2. Two shared root causes: taint enters only
through tool outputs marked untrusted, and the secret-read gate is a name denylist.

## 4. PR #35 (messaging channel)

| Gate | Result |
|---|---|
| Exact-head hosted CI (merge ref `6e80e18`, 3 OS) | PASS |
| Local CI on #35 + main + #39 (`310f3ba0`) | PASS (279 tests, incl. the Telegram integration test) |
| Independent security review | Done: no P0; P2 C35-1, C35-2, C35-3; P3 C35-4..7. Owner's own pre-merge gate on credential policy still open (no tests for token storage failure, rotation, disconnect). |
| Live Telegram integration | BLOCKED: needs a disposable bot token and test account |
| Desktop startup and Settings smoke test | Headless layout PASS; real window NOT_RUN |
| Draft status | Draft. Not mergeable under the mission rules. |

**Not merged.** Needed: owner decision on C35-8 (cap phone turns at Ask), fixes for C35-1/2/3,
live bot test, ready-for-review.

## 5. PR #26 (OAuth)

| Gate | Result |
|---|---|
| Exact-head hosted CI (merge ref `1cc9dc7`, 3 OS) | PASS |
| Local CI on #26 + main + #39 (`a557aac1`) | PASS (271 tests) |
| Independent security review | Done: P1 O26-1 (provider calendar text untainted), O26-2 (only recovery from a revoked token deletes data); P2 O26-3..7. PKCE, state, redirect, vault-only storage verified sound. |
| Provider interoperability (Google, Microsoft) | BLOCKED: needs nonproduction OAuth app registrations and test accounts |
| Credential storage | macOS Keychain round trip PASS locally and on hosted macOS/Windows; Linux deliberately refused |
| Native UI | 4 new callbacks wired (static check); real window NOT_RUN |

**Not merged.** Needed: fixes for O26-1/O26-2, provider credentials for the live tests,
ready-for-review.

## 6. Merge history

None. No PR was merged, nothing was pushed to `main`, to #26 or to #35, and no tag was created.
This run opened three PRs (#39 test isolation, #40 read gate, #41 mutation-gap tests) and pushed
this qualification branch.

## 7. Security findings and fixes

Report 07. Fixed in PRs by this run: M-5 and part of M-4 (#40), M-TEST-1 (#39), mutation gaps in
the shell classifier and SSRF guard (#41). Left for owner
decisions: M-1, M-2, M-3/O26-1, M-6, C35-8, rest of M-4. Dependency: RUSTSEC-2026-0459 in
bundled SQLite (M-DEP-1), fix by upgrading `rusqlite` to 0.41.

## 8. Local CI results

Report 03. `scripts/verify-local.sh` mirrors `ci.yml` and writes per-step evidence. First pass:
`tests` failed on all four trees because the suite assumed UTC and no provider key (M-TEST-1),
which also hid the integration tests. With #39: all required steps pass on all three PR trees.
Advisory: fmt fails (D-7), clippy `-D warnings` fails (56 on `main`), cargo-audit and
cargo-deny fail on M-DEP-1.

## 9. Mutation testing

Report 04. testteeth over cargo-mutants, `src/permissions.rs` only, unsampled: 223 viable
mutants, **69.1%** on `main` + #39. Survivors included shell-classifier and SSRF-guard gaps that
could let a program-starting command count as read-only or an internal address count as public.
This run added tests (PR #41): **87.9%**, 43 more mutants killed, each targeted mutant also
checked by hand. The SSRF bypass switches (`HERON_TRUST_WEB_PROXY`, `HERON_ALLOW_LOCAL_FETCH`)
remain untested. Telegram, connectors, config, `channel.rs` (#35) and `oauth.rs` (#26) were not
mutation-tested, so the 80% critical-module gate is not met.

## 10. Genesis evaluations

NOT_RUN. An honest agent evaluation needs an approved model credential and budget. Report 05
lists the deterministic scripted-model evidence and a ready-to-build evaluation design.

## 11. Native Slint UI

Report 06. Builds, desktop tests and headless rendering pass at four sizes. Every root callback
has a Rust handler. Real-window, keyboard, screen-reader and scaling checks were NOT_RUN. One P2:
selected nav item and selected mode chip are close to illegible (M-UI-1). A separate session is
improving the UI at the owner's request.

## 12. Reliability and recovery

Report 08. PARTIAL: invariants for restart, duplicate approvals, damaged files and migrations have
passing tests. Kill-during-upgrade, disk-full, real upgrade/rollback and recovery timing were not
run. PR #35 and #26 each break one invariant (silent reply loss; per-account erasure).

## 13. Cross-platform matrix

Report 09. macOS: local and hosted build/test. Windows, Linux: hosted build/test only. No
platform: installer, upgrade, real window, signing.

## 14. Release artifacts

NOT_RUN. A release candidate is prepared only after the engineering gates pass; they did not.
No `v1.0.0-rc.1` tag, build or changelog entry was made. Pushing any `v*` tag would trigger
`release.yml` and publish artifacts.

## 15. Remaining blockers

| Blocker | Who clears it |
|---|---|
| M-1 P0 and the P1 policy findings | Owner decision on taint and prompting rules (§17), then an implementation PR |
| C35-1/2/3, O26-1/2 | PR authors |
| Live Telegram test | Owner: disposable bot token + test account |
| Live Google/Microsoft OAuth tests | Owner: nonproduction app registrations + test accounts |
| PR #35 and #26 are drafts | Owner |
| Branch protection and required checks | Repository admin |
| Signing / notarization | Apple Developer ID; Windows code-signing certificate |
| Windows/Linux runtime, installer and upgrade drills | A Windows and a Linux machine or qualified runners |
| M-DEP-1 | `rusqlite` 0.41 upgrade PR |
| Agent evaluation | Approved model budget |

## 16. Known limitations of this run

- Only macOS was available. No real-window UI testing (GUI launch is blocked in this sandboxed session).
- The reviewer reports for #35 and #26 were not re-executed line by line; their P0/P1 claims on
  `main` were. PR-specific P1s are TRACED, not reproduced.
- M-1 was observed as a policy decision allowing `web_fetch` to `collector.invalid`; no data left
  the machine.
- M-REL-1's cause is reasoned, not reproduced.

## 17. Go/no-go

**No-go.** Decisions only the owner can make:

1. **M-1 / D-2.** Keep D-2 as is (and state the credential-read-then-egress consequence), or
   (a) taint conversations on shell output and on reads outside the workspace, or (b) ask before
   model-built egress once any non-user content was read.
2. **Remote channels (C35-8).** Cap phone turns at Ask regardless of the desktop mode?
3. **System-prompt context (M-3, O26-1, M-6).** Taint conversations that include calendar
   titles, unconfirmed memories or proactive prompts, or stop including them?
4. **MCP server callers (M-2).** Treat them as outside content?
5. **Release scope.** Are #35 and #26 in v1.0? Is v1.0 macOS-only?

## 18. Exact next actions

1. Review and merge #39 (tests only), #40 (read gate) and #41 (tests only) once hosted CI is green.
2. Decide items 1 to 5 above.
3. Provide a disposable Telegram bot and nonproduction OAuth apps, then rerun §5.4 and §6.5.
4. Turn on branch protection requiring the six CI checks.
5. Upgrade `rusqlite` to 0.41 in its own PR.
6. Rerun `scripts/verify-local.sh` and this qualification against the new `main`.
