# 03. CI equivalence and local CI results

Runner: `scripts/verify-local.sh`. It mirrors `.github/workflows/ci.yml` (the `test` job) step for step,
records each step to `<evidence_dir>/evidence.jsonl` (command, exit code, UTC start/end, commit
and tree SHA, platform, rustc, test counts), and exits non-zero when a required step fails.
Advisory steps (fmt, clippy, cargo-audit, cargo-deny) are not in hosted CI; they fail the run only
with `--strict`. One deliberate difference: local `cargo test` uses `--no-fail-fast`, so one failing
test binary cannot hide the others (the first pass below did not have it).

Host: macOS 27.2 arm64, rustc 1.99.1, `HERON_TEST_BROWSER=1`, run outside the Claude Code sandbox
because the steps need the Keychain, a real browser and local ports. Evidence records:
`qualification/evidence/ci/<sha>/evidence.jsonl`. Raw logs stay out of Git in each worktree's
`target/qualification/<sha>/`. The copied evidence was checked for key material before commit.

## Trees tested

Merge commits were made locally and not pushed. The local merges of #35 and #26 onto `449231f`
have the same tree SHAs as GitHub's `refs/pull/35/merge` (`bdd16e29…`) and `refs/pull/26/merge`
(`d28adef1…`), so hosted results for those merge refs apply to exactly these trees. Hosted run
43954482420 tested `6e80e18` (#35 into `449231f`); hosted run 43929044780 tested `1cc9dc7`
(#26 into `449231f`), read from the "HEAD is now at" log lines.

| Tree | Commit | tests | vault-live | desktop-build | desktop-tests | ui-snapshot | linux-install-smoke | fmt | clippy | cargo-audit | cargo-deny |
|---|---|---|---|---|---|---|---|---|---|---|---|
| main | `449231fcbef2` | FAIL (231p/2f/1i) | PASS | PASS | PASS | PASS | NOT_APPLICABLE | FAIL | FAIL | NOT_RUN in runner¹ | NOT_RUN in runner¹ |
| #35+main | `befed831a3ed` | FAIL (245p/2f/1i) | PASS | PASS | PASS | PASS | NOT_APPLICABLE | FAIL | FAIL | NOT_RUN¹ | NOT_RUN¹ |
| #26+main | `a971f7479321` | FAIL (237p/2f/1i) | PASS | PASS | PASS | PASS | NOT_APPLICABLE | FAIL | FAIL | NOT_RUN¹ | NOT_RUN¹ |
| #35+#26+main | `4e60e2906c1a` | FAIL (251p/2f/1i) | PASS | PASS | PASS | PASS | NOT_APPLICABLE | FAIL | FAIL | NOT_RUN¹ | NOT_RUN¹ |
| #35+main+#39 | `310f3ba07f95` | PASS (279p/0f/1i) | PASS | PASS | PASS | PASS | NOT_APPLICABLE | FAIL | FAIL | FAIL | FAIL |
| #26+main+#39 | `a557aac14868` | PASS (271p/0f/1i) | PASS | PASS | PASS | PASS | NOT_APPLICABLE | FAIL | FAIL | FAIL | FAIL |
| #35+#26+main+#39 | `97a418e48424` | PASS (285p/0f/1i) | PASS | PASS | PASS | PASS | NOT_APPLICABLE | FAIL | FAIL | FAIL | FAIL |

¹ The tools were not installed when the first pass ran (the evidence file says NOT_APPLICABLE for
that reason). They were then run by hand on `main`'s lockfile: `cargo audit` exit 1, `cargo deny
check advisories` exit 1 (logs in `target/qualification/449231f…/`). Neither PR changes
`Cargo.lock` or `Cargo.toml`, so the dependency result is the same on every tree.

### Why `tests` failed on the first pass

Not a product failure. `main`'s suite assumes the host is in UTC with no `OPENROUTER_API_KEY` set
(finding M-TEST-1). Two `ui::views` tests fail at UTC+8, and because plain `cargo test` stops at
the first failing binary, the integration tests (including #35's changed Telegram test) never ran.
PR #39 fixes the tests; with it merged, every required step passes on all three PR trees. On
#39's own branch the full suite also passes in `TZ=Asia/Singapore` with the key variable set
(231 lib, 9 integration, 22 Slint logic).

### Advisory results

- `cargo fmt --check`: fails (known baseline, DECISIONS D-7).
- `cargo clippy --all-targets --features desktop -D warnings`: 56 errors on `main`, mostly
  `field_reassign_with_default` (32). No `await_holding_lock` hit in this run.
- `cargo audit`: 1 vulnerability, RUSTSEC-2026-0459 in `libsqlite3-sys` 0.30.1 (finding M-DEP-1),
  plus 8 unmaintained-crate warnings (`bincode`, `zvariant*` and others).
- `cargo deny check advisories`: fails on the same SQLite advisory and on `chrono` marked
  unmaintained (RUSTSEC-2026-0543). The repository has no `deny.toml`; defaults were used.

## Hosted CI, as read from GitHub

| SHA | Test ubuntu-22.04 | Test macos-latest | Test windows-latest | Neural (3 OS, informational) |
|---|---|---|---|---|
| `449231f` main | success | success | success | success |
| `6e80e18` #35 merge | success (246 lib passed) | success (245 passed, 1 ignored) | success (218 passed, 1 ignored) | success |
| `1cc9dc7` #26 merge | success | success | success | success |
| `18e798f` PR #39 | **failure**: `connectors::tests::registry_is_atomic_private_and_never_contains_keys`, "Connector registry is busy in another HERON instance" | success | success | success |

The PR #39 Linux failure is in a test #39 does not touch. Probable cause (reasoned, not
reproduced): `lock_registry` uses a non-blocking `flock` (`src/connectors.rs:99`); on Linux a child
process spawned by a parallel test briefly shares open file descriptions between fork and exec,
so a just-closed lock can still be held when the test reopens it. macOS spawns without that
window. Recorded as finding M-REL-1 (P2, flaky on Linux; in the product a registry change made
while Heron spawns a subprocess could report "busy" spuriously). The failed job was re-run and
passed, so all six checks on #39 are green. On #40 (`448227d`) the two macOS jobs were cancelled
by the runner (no step failed, no test output) and were re-run.

## Equivalence

| CI step | Local counterpart | Classification |
|---|---|---|
| `cargo test --locked` (3 OS) | `tests` | Fully reproduced on macOS. Windows and Linux require those OSes: hosted evidence only. |
| Native vault round trip (macOS, Windows) | `vault-live` | Fully reproduced on macOS (real Keychain). Windows: hosted only. |
| Desktop build | `desktop-build` | Fully reproduced on macOS. |
| Desktop bin tests | `desktop-tests` | Fully reproduced on macOS. |
| Headless Slint snapshot (Linux) | `ui-snapshot` | Reproduced on macOS (CI runs it on Linux only); BMPs captured at 1100x700, 800x600, 640x480, 1440x900. |
| Linux installer smoke test | `linux-install-smoke` | Requires Linux: NOT_APPLICABLE here, hosted only. |
| Neural memory job | not mirrored | Informational in CI (`continue-on-error`); not run locally. |
| Real-browser test | inside `tests` with `HERON_TEST_BROWSER=1` | Reproduced on macOS with the installed browser. |

macOS success here is not Windows or Linux verification.
