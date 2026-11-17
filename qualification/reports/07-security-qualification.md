# 07. Security qualification

**Result: FAIL.** One P0 and six P1 findings are open on `main`, all reproduced or traced to code.
The full register with locations is in report 02; raw reviewer reports are in
`qualification/evidence/reviews/`; executable reproductions are in `qualification/evidence/repro/`.

## Trust boundaries

| Boundary | State on `main` | Findings |
|---|---|---|
| User ↔ desktop app | Loopback API with per-launch token, constant-time compare, Host/Origin checks. Sound. | D-3/T9 accepted: the app's own buttons bypass the mode. |
| App ↔ model provider | Keys from env or OS vault; never in config, DB or command env. Sound in review. | Linux keeps legacy secrets in an owner-only file. |
| Model output ↔ tool executor | Typed effects, decision table tested. Gaps in what counts as outside content and in the secret-read list. | **M-1 (P0)**, M-4, M-5 (fixed in PR #40), M-7 |
| Remote messaging ↔ agent | Owner pairing, private chats only, challenge-coded approvals, taint persists across phone turns. | **C35-8 (P1)**: phone turns inherit Auto/Yolo; M-8 suspected |
| OAuth provider ↔ vault (PR #26) | PKCE S256, single-use state, vault-only tokens, Linux refused. Sound in review. | O26-2 (P1), O26-3..7 (P2) |
| External content ↔ memory | Outside sources unconfirmed by default. | **M-2**, **M-3**, O26-1, O26-5 |
| MCP server ↔ privileged actions | Mode applies, but callers are treated as clean. | **M-2** |
| Plugin/skill ↔ filesystem | Skills opened with `O_NOFOLLOW`, size-bounded (PR #36). Sound in review. | none |
| Background work ↔ approval policy | Triggered jobs start tainted; proactive suggestions do not. | **M-6**, M-7 |

## Fixes made by this run

| PR | Head | Fixes | Status |
|---|---|---|---|
| [#40](https://github.com/fernandogarzaaa/HERON-Project/pull/40) | `448227d941d4e3ceb348e701a25ee4b811509244` | M-5 (search through a link to a secret) and the Heron-home part of M-4 | Open. Local suite passes; tests fail without the fix. Hosted CI pending at time of writing. |
| [#39](https://github.com/fernandogarzaaa/HERON-Project/pull/39) | `18e798f5217ea51f5d123e761dbb6d7db02e4492` | M-TEST-1 (test isolation), which was hiding the integration suite | Open. macOS and Windows hosted green; Linux failed on unrelated flaky M-REL-1; re-run requested. |

## Not fixed, and why

M-1, M-2, M-3/O26-1, M-6, C35-8 and the glob/quote part of M-4 all change *when users are asked*
or *what counts as outside content*. DECISIONS.md shows the owner sets those rules (D-2, D-3,
D-4). This run wrote failing reproductions and options instead of changing the policy. The
options are in report 02 and in the final report's decision list.

## Dependency vulnerabilities

RUSTSEC-2026-0459 (bundled SQLite, CVSS 6.8, local information disclosure via a crafted
`-wal` file). Reachable through `heron restore` of an untrusted backup. Fix: upgrade `rusqlite`
to 0.41 (needs Rust 1.89; the toolchain here is 1.99). Not attempted in this run: a major
`rusqlite` upgrade touches `store.rs` (1,930 lines) and needs its own PR and test pass.

## Checked and sound

Local API authentication; web-fetch SSRF guard (connect-time public-only resolver, redirect and
downgrade rules); Telegram owner check and pairing lock; write path canonicalisation and
`cfg.home` protection; shell environment allowlist; MCP tool fingerprinting; PKCE and state
handling in #26; token-free logging in #35 and #26; skill loading.

## Not examined

`cli_llm.rs` beyond argv construction, `mail.rs` beyond TLS setup, browser internals, migrations
(covered by existing tests), and the Slint desktop code for security issues.
