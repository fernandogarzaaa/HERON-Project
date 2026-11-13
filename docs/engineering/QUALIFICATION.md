# Qualification

**Label: BETA / NOT PRODUCTION QUALIFIED.**

## How to reproduce the checks

Environment used for the figures in this folder: Linux 6.18 (x86_64), rustc 1.97.0, Chromium 1243
for the browser test.

```bash
cargo test                                   # 105 unit + 8 integration (browser test skipped)
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
