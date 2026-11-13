# Qualification

**Label: BETA / NOT PRODUCTION QUALIFIED.**

## How to reproduce the checks

Environment used for the figures in this folder: Linux 6.18 (x86_64), rustc 1.97.0, Chromium 1243
for the browser test.

```bash
cargo test                                   # 121 unit + 8 integration (browser test skipped)
```
```bash
HERON_TEST_BROWSER=1 cargo test              # also drives the installed browser
```
```bash
cargo build --features desktop --bins        # the desktop app
```
```bash
cargo test --features neural --lib -- --ignored neural   # optional neural memory
```
```bash
cargo check --target x86_64-pc-windows-gnu --tests       # Windows type-check from Linux
```

Hosted CI (GitHub Actions, `ci.yml`) runs the first three on Linux, macOS and Windows, and the
neural test on all three. A local pass is not a hosted pass; the pull request shows which
commit CI ran on.

## Measured baseline that is not yet clean

| Check | Result | Consequence |
|---|---|---|
| `cargo fmt --all -- --check` | 946 diff hunks | Not a gate yet (DECISIONS D-7) |
| `cargo clippy --all-targets` | 39 warnings; one is a lock held across an await | Not a gate yet |
| `cargo audit`, `cargo deny` | Not installed here; never run | Dependency advisories are unchecked |
| Mutation testing, fuzzing | Never run | No kill-rate figures exist |

## What has been run where

| Thing | Linux | macOS | Windows |
|---|---|---|---|
| Test suite | Local and CI | CI | CI |
| Real-browser test | Local (Chromium) and CI | CI (Chrome) | CI |
| Command sandbox test | CI (skipped where bubblewrap is absent) | CI (Seatbelt) | No sandbox exists |
| v0.1.0 installer | Installed and run | Built, never run | Built, never run |
| Real model | Two free OpenRouter models, by hand, a handful of tasks | — | — |

## External blockers and how to clear them

1. **Signing.** Needs an Apple Developer ID (notarisation) and a Windows code-signing
   certificate, stored as repository secrets. Until then installers are unsigned.
2. **Branch protection.** Needs a repository admin: Settings → Rules → Rulesets → New branch
   ruleset on `main`; require a pull request, require the six CI checks
   (`Test (ubuntu-22.04)`, `Test (macos-latest)`, `Test (windows-latest)` and the three
   `Neural memory` jobs), block force pushes.
3. **Stray release asset.** `index.html` is attached to v0.1.0. Delete it on the release page;
   the workflow is already fixed for future releases.
4. **OAuth connectors.** Need app registrations with Google and Microsoft and accounts owned by
   the tester.
5. **Paid models.** The OpenRouter account has no credit, so only free models were tested.

## Gates before the label can change

- Every "Not started" security item in REMEDIATION_MATRIX.md sections A and B is Verified or
  has a written risk acceptance from the owner.
- Undo conflict detection and a durable job queue (C2, C3).
- Formatting, lint and advisory gates passing on the release commit.
- Signed installers run on real macOS and Windows machines.
- The owner's written sign-off.

## Upgrade from the published release, run by hand (13 November 2026)

Not a hosted check; one run on Linux x86_64.

1. Downloaded `Heron-linux-x64.tar.gz` from the v0.1.0 release and checked it against `SHA256SUMS.txt`.
2. With that binary, in demo mode and an empty home, created four conversations, a reminder, a task
   and a file edit (three activity-log entries, one checkpoint). The database had no format number
   and none of the newer tables.
3. Ran this branch's `heron doctor` on the same home. Result: `data format 2, checked: sound`; one
   copy in `backups/` named `heron-before-upgrade-from-v0-…`, mode 0600 in a 0700 folder; the
   reminder, task, four conversations and the three verified log entries all listed by the new build.
4. `heron backup`, added a task, `heron restore <that backup>`: the added task was gone and the
   previous state was saved as `heron-before-restore-…`.
5. `heron restore` of the pre-upgrade copy (released format): restored, upgraded again, sound.
6. A text file and a missing file were refused as restore sources.
7. The published 0.1.0 binary opened the upgraded data and listed the task. It has no format check;
   this works only because format 2 adds tables and a column and changes nothing that existed.

8. Overwrote the middle third of the database file, removed its log files, and ran `heron doctor`
   (reports the damage) and then `heron restore <backup>`: restored and sound; the damaged file
   kept as `heron-damaged-…`.

Not done: macOS or Windows by hand; a database with thousands of conversations; an upgrade
interrupted by killing the process (the transaction is what protects it; only the in-process
failure is tested).
