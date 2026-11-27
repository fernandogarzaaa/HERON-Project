# 09. Platform matrix

Supported platforms claimed by `README.md`: macOS (Apple silicon and Intel, 11+), Windows 10/11 x64,
Linux x86_64. This run had one machine: macOS 27.2 on Apple silicon. Hosted GitHub Actions runs
are cited only for the SHAs they ran on, and only for the steps they contain.

Result words: PASS (command, exit code and SHA on record), FAIL, BLOCKED (cannot run here; the
missing thing is named), NOT_RUN.

| Check | macOS arm64 (this Mac) | macOS (hosted `macos-latest`) | Windows x64 (hosted) | Linux x64 (hosted ubuntu-22.04) |
|---|---|---|---|---|
| Clean build, `cargo test --locked` on `main` `449231f` | see report 03 | PASS (CI run on `449231f`) | PASS (CI) | PASS (CI) |
| Native credential vault round trip | see report 03 | PASS (CI) | PASS (CI) | NOT_APPLICABLE: Linux uses an owner-only file, no native vault gate exists |
| Desktop build and bin tests | see report 03 | PASS (CI) | PASS (CI) | PASS (CI) |
| Headless Slint render | see report 03 | NOT_RUN in CI | NOT_RUN in CI | PASS (CI snapshot artifact) |
| Desktop app launched as a real window | NOT_RUN (see report 06) | NOT_RUN | NOT_RUN | NOT_RUN |
| Installer: install | NOT_RUN: needs a signed build and a clean user account | NOT_RUN | NOT_RUN | PASS for the script only (`tests/linux-install-smoke.sh`, fixture binaries) |
| Upgrade from 0.3.x with real data | NOT_RUN | NOT_RUN | NOT_RUN | NOT_RUN (store upgrade unit tests cover the database format only) |
| Uninstall | NOT_RUN | NOT_RUN | NOT_RUN | PASS for the script only |
| Signing / notarization | BLOCKED: no Apple Developer ID | BLOCKED | BLOCKED: no Authenticode certificate | NOT_APPLICABLE |

Cross-compilation was not used as runtime evidence.

## Verdict for the matrix

Windows and Linux have hosted build-and-test evidence only. No platform has installation,
upgrade, real-window, or signed-artifact evidence. The matrix does not pass. If v1.0 is
narrowed to macOS, that is a release-scope decision for the owner; this run does not make it.
