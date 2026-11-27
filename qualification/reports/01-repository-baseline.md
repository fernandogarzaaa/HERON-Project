# 01. Repository baseline

Recorded 2026-11-17T17:40Z (UTC) from a MacBook Pro (Apple silicon), by the v1.0 qualification run.
Every SHA below is the full 40-character object name, read with `git rev-parse` or the GitHub API.

## Environment

| Item | Value |
|---|---|
| Hardware / OS | arm64, macOS 27.2 (build 26B123), Darwin 27.2.0 |
| Xcode | `/Applications/Xcode.app/Contents/Developer`, SDK 27.2 |
| Rust | rustc 1.99.1 (58cf8f9c3 2026-10-13), cargo 1.99.1. No `rust-toolchain` file pins a version; CI uses `stable`. |
| Other | git 2.56.0, gh 2.103.2, Python 3.12.15, Node 26.13.0, Docker 29.8.2, cargo-mutants 27.1.0, testteeth, genesis 0.4.0 |
| Not preinstalled | cargo-audit, cargo-deny (installed into a temporary root for this run; see report 03) |

## Revisions

| Thing | Value |
|---|---|
| Default branch | `main` |
| `main` head | `449231fcbef29726a18aafe5937fa197b9e80c0a` (tree `e7e659a088141b6909d2062b3d7982126d8d073e`), "Merge PR #38", committed 2026-11-17T02:24:04+08:00 |
| Package version | `heron` 0.3.1 (`Cargo.toml`, `Cargo.lock`), CHANGELOG has an empty `Unreleased` section |
| Tags | `v0.1.0`, `v0.2.0`, `v0.3.0`. No `v0.3.1` tag although 0.3.1 is in the changelog. |
| GitHub releases | v0.1.0, v0.2.0, v0.3.0, all pre-release (beta) |
| Qualification branch | `qualification/v1.0-rc1`, cut from `main` at `449231f` |

## Open pull requests

| PR | Head SHA | Base SHA recorded by GitHub | Draft | Mergeable | Merges onto current main |
|---|---|---|---|---|---|
| #35 channels | `1c348ab93323950e4a0d57fd2b8548ff75bb6f25` | `eae4cb30cf0cb9ec951877aeabeff10ed46234ca` | yes | MERGEABLE | clean (local merge `befed831a3ed879efc51ee5a3055adb1a3daab42`) |
| #26 OAuth | `183f3da50137d41f07e17fd81940e1df761ddb04` | `eae4cb30cf0cb9ec951877aeabeff10ed46234ca` | yes | MERGEABLE | clean (local merge `a971f7479321dfc4807459817f64274197746126`) |
| #35 then #26 | | | | | clean (local merge `4e60e2906c1a7492e9027458ad1d93472d0dc2ab`, auto-merged `src/lib.rs`, `src/server.rs`) |

Both heads match the SHAs given in the mission brief, so neither PR moved since it was last observed.
Both bases are stale: `main` gained #36 and #38 after `eae4cb3`. The local merge commits above are
the integration trees this run tested; they exist only in local worktrees and were not pushed.

Diff size against `main`: #35 changes 6 files (+1164 −47; new `src/channel.rs`, large `src/telegram.rs`
change). #26 changes 10 files (+1133 −20; new `src/oauth.rs`, server, sources, store, memory, Slint
settings).

## Recently merged

| PR | Merged (UTC) | Merge commit | Title |
|---|---|---|---|
| 38 | 2026-11-16T18:24:04Z | `449231fcbef29726a18aafe5937fa197b9e80c0a` | consent-bound connector event provenance and budgets |
| 36 | 2026-11-16T18:23:44Z | `8fdb4755f02a2bba0c9db66c827b70dc8c77f89b` | bounded skill loading and provenance-ready manifests |
| 33 | 2026-11-15T01:40:53Z | `d854755159feacda891f9731b3b949765bece241` | bound server, desktop and UI SSE backpressure |
| 32 | 2026-11-15T00:39:14Z | `a7ce6d0c1c7bb8e995ea5c6ce623deef3368187a` | explain decisions and block unattended MCP tool execution |
| 31 | 2026-11-16T05:22:11Z | `eae4cb30cf0cb9ec951877aeabeff10ed46234ca` | connector catalog, custom registry and OS-vault-only credentials |
| 30 | 2026-11-14T23:45:48Z | `f9cb11ca90d16135073135ce88ad1c4bb3091944` | fail-closed web-fetch DNS/proxy and redirect SSRF guards |

## Branch protection and required checks

`GET /repos/fernandogarzaaa/HERON-Project/branches/main/protection` returns 404 "Branch not protected",
and `GET .../rulesets` returns `[]`. Main has no required reviews, no required status checks, and no
force-push block. Any statement that "required reviews and checks are satisfied" is therefore vacuous
on this repository. This is finding **M-GOV-1** (P1 for a production release; it was already listed
as external blocker G6 in `docs/engineering/QUALIFICATION.md`). Changing repository settings is
outside this run's authority.

## CI workflows

- `.github/workflows/ci.yml`: one `test` job on ubuntu-22.04, macos-latest and windows-latest
  (cargo test, native vault round trip on macOS/Windows, desktop build, desktop bin tests, Linux
  headless UI snapshot and installer smoke test), plus an informational `neural` job with
  `continue-on-error: true`. No fmt, clippy, audit, deny, mutation or fuzz step.
- `.github/workflows/release.yml`: triggered by any pushed `v*` tag (and a one-time
  `release/v0.3.0-publish` branch). Builds and uploads macOS, Windows and Linux artifacts.
  Consequence for this run: **no `v*` tag may be pushed**, because a tag publishes a release.

Hosted CI conclusions read from the GitHub checks API:

| SHA | Test (3 OS) | Neural (3 OS) |
|---|---|---|
| `449231f` main | success | success |
| `1c348ab` #35 (run 43954482420 tested merge `6e80e18` = #35 into `449231f`) | success | success |
| `183f3da` #26 (run 43929044780) | success | success |

The 2m31s duration shown for run 43954482420 is the last job's re-run; the Test jobs ran
20:53Z to 21:02Z and their logs show 245 to 246 library tests passing per OS.

## Entry points and test infrastructure

- Binaries: `heron` (`src/main.rs`, CLI and local server), `heron-desktop` (`src/bin/desktop/main.rs`,
  Slint, feature `desktop`).
- Source: about 32,000 lines of Rust in `src/`, Slint UI in `ui/`.
- Tests: unit tests in each module, `tests/integration.rs` (scripted model server, HTTP API, MCP,
  Telegram gateway, real browser), `tests/slint_setup_logic.rs`, desktop bin tests including a
  headless layout/render test, `tests/linux-install-smoke.sh`.
- Existing qualification documents (written 13 November against an older `main`):
  `docs/engineering/{QUALIFICATION,FINAL_QUALIFICATION_REPORT,REMEDIATION_MATRIX,SECURITY_THREAT_MODEL,DECISIONS,TEST_STRATEGY}.md`.
  This run treats them as claims and reconciles with them rather than replacing them.

## Local state preserved

The primary checkout `<repo>` is on branch
`fix/macos-heron-cli-collision` at `023ead7`. This run did not touch it.
