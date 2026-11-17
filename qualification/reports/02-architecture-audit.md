# 02. Architecture audit and findings register

Audited revision: `main` `449231fcbef29726a18aafe5937fa197b9e80c0a`, plus PR #35 head
`1c348ab93323950e4a0d57fd2b8548ff75bb6f25` and PR #26 head `183f3da50137d41f07e17fd81940e1df761ddb04`.
Method: three independent read-only reviews (raw reports in `qualification/evidence/reviews/`),
then each P0/P1 claim on `main` reproduced by this run in a clean `git archive` export
(`qualification/evidence/repro/`). Line numbers are at the audited revision of each item.

## 1. Component map

| Area | Where | Notes |
|---|---|---|
| Agent loop | `src/agent.rs` | Builds the system prompt (profile memories, calendar lines, skills), runs tool calls through `call_tool`, tracks per-conversation taint in `Session.tainted`. |
| Permission policy | `src/permissions.rs` | Typed `Effect` (10 kinds) × `Mode` (ReadOnly, Ask, Auto, Yolo) × taint. `classify_path`, `classify_shell`, SSRF guard. Decision table test covers all cells. |
| Tools | `src/tools/{fs,shell,web,personal,mod}.rs`, `src/browser.rs`, `src/mcp.rs` | Each tool declares an effect function. Shell runs in Seatbelt (macOS) / bubblewrap (Linux), none on Windows. |
| App wiring | `src/app.rs` | Scheduled jobs, triggered jobs, proactive suggestions, parked approvals, background-work lock. |
| Persistence | `src/store.rs` (SQLite, numbered migrations, backup/restore), `src/memory.rs`, `src/proactivity_events.rs` | `rusqlite` 0.32 with bundled SQLite. |
| Credentials | `src/config.rs` (`store_secret`, vault-only helpers), `src/credential_transaction.rs`, `src/connectors.rs` | macOS Keychain and Windows Credential Manager via `keyring`. Linux: owner-only `secrets.json` for legacy secrets; vault-only paths refuse Linux. |
| Local API | `src/server.rs` (axum, loopback, per-launch token, Host/Origin checks) | Desktop and web UI talk to it. |
| Messaging | `src/telegram.rs` (main); PR #35 adds `src/channel.rs` durable spool | Owner pairing, private chats only, phone approvals with challenge codes. |
| OAuth | PR #26 only: `src/oauth.rs`, server routes, Slint settings | Google and Microsoft 365, PKCE, read-only scopes. |
| Desktop | `src/bin/desktop/*.rs`, `ui/**/*.slint` (Slint 1.18.1) | 65 root callbacks on `main`, 69 with #26. A static check found every one bound in Rust (report 06). |

## 2. Capability classification

| Capability | State on `main` | Basis |
|---|---|---|
| Remote model providers (OpenAI-compatible HTTP, subscription CLIs) | Implemented, tested with a scripted model; real-provider runs by hand only | `tests/integration.rs`, `QUALIFICATION.md` |
| Local inference (localhost endpoint) | Implemented, configuration-tested only | `is_local` in `server.rs` |
| Tool calling and permission enforcement | Implemented and verified for the policy table; boundary gaps below (M-1, M-4, M-5) | `permissions::tests::the_decision_table`, repro tests |
| User approvals | Implemented and verified (atomic claim, phone challenge codes) | `app::tests`, Telegram integration test |
| MCP client and server | Implemented; server path treats callers as clean (M-2) | integration test, repro `m2` |
| Browser control | Implemented, local test site only; no request interception (T4) | integration test |
| Memory | Implemented, deterministic retrieval eval exists | `heron eval` |
| Background tasks and schedules | Implemented, atomic claims, single owner lock | `store` tests |
| Proactivity | Implemented, off by default; runs model-written prompts in a clean session (M-6) | repro `m6` |
| Messaging (Telegram) | Implemented on `main`; tested against a stand-in API only; PR #35 adds a durable queue | integration test, report on #35 |
| OAuth (Google, Microsoft) | Not on `main`; PR #26 draft; no live provider test | review O26 |
| Credential storage | macOS/Windows vault verified on hosted and local runners; Linux falls back to an owner-only file | `connector_vault_live_round_trip` |
| Voice, email sending, calendar writing | Missing (documented in README) | README "Known limits" |

## 3. Findings register

Status tags: **NEW** (not in the repo's documents), **KNOWN** (open item T-n in
`SECURITY_THREAT_MODEL.md` or an item in `REMEDIATION_MATRIX.md`), **ACCEPTED** (owner risk
acceptance D-n in `DECISIONS.md`). Verification: **REPRO** (this run executed a test that shows
it), **TRACED** (a reviewer followed the code end to end; not executed), **SUSPECTED**.

### P0

| ID | Component | Location | Finding | Tag | Verification | Fix |
|---|---|---|---|---|---|---|
| M-1 | Permissions | `src/tools/fs.rs:788,828` (`untrusted: false`), `src/permissions.rs:256,602` | In the default Ask mode, injected text in a workspace file steered the model to read a credential file outside the workspace (`~/.config/gh/hosts.yml`, classified `LocalRead`) and to call `web_fetch` with the token in a model-built URL. Zero approval prompts; audit log shows `read_file -> allowed`, `web_fetch -> allowed`. The target was `collector.invalid`, so this run observed the policy permit the call, not data leaving the machine. | ACCEPTED in part (D-2 allows egress in a "clean" conversation; README "Known limits" says files the user asks Heron to read are not outside content). The credential-read-then-egress consequence is not stated in either. | REPRO (`main-trust-boundary-repro.rs::m1`) | Owner decision. Options: (a) taint on shell output and on reads outside the workspace; (b) ask for model-built `NetworkEgress` once any non-user content was read in the conversation; (c) keep D-2 and state the consequence. PR #40 narrows M-4/M-5 but does not close M-1. |

### P1

| ID | Component | Location | Finding | Tag | Verification | Fix |
|---|---|---|---|---|---|---|
| M-2 | MCP server | `src/mcp.rs:266` (`decide_in(..., false)` next to a comment saying MCP input is outside content); `src/memory.rs:18` | Another program connected over MCP planted a confirmed core profile memory (source `agent`) and an unattended daily schedule whose prompt reads a file and fetches a URL. | NEW (D-5 covers MCP client tools, not the server) | REPRO (`m2`) | Owner decision on taint semantics; candidate: store MCP writes as source `mcp` in `OUTSIDE_SOURCES`, start MCP-created schedules tainted or unconfirmed. |
| M-3 | Agent prompt | `src/agent.rs:287` (calendar titles), `:250` (unconfirmed memories) | Calendar event titles (anyone can send an invite) go into the system prompt of every conversation without tainting it. PR #26 extends this to Google and Microsoft calendars (O26-1). | KNOWN, wider than documented (T2) | REPRO (`m3`) | Owner decision: taint when included, or serve through `search_knowledge` which taints. |
| M-4 | Secret-read gate | `src/permissions.rs:511,538`; `src/tools/fs.rs:16` | Shell globs/quotes/backslashes (`cat .en?`, `cat .e""nv`) and unlisted credential paths (`~/.config/gh/hosts.yml`, `~/.cargo/credentials.toml`, shell histories, browser `Login Data`, a custom `HERON_HOME/secrets.json`) classify as `LocalRead`. | KNOWN (T7) | REPRO (`m5`) | Part fixed by PR #40 (anything under Heron's home). Remaining: glob/quote rule (changes prompting, owner decision) and list extension. |
| M-5 | `search_files` | `src/tools/fs.rs:707` | A workspace link `notes.txt -> ~/.ssh/id_ed25519` was grepped and its contents returned without approval; `read_file` on the same link asks. | NEW | REPRO (`m4`) | **Fixed in PR #40** (`448227d`), regression tests fail without the fix. |
| M-6 | Proactivity | `src/app.rs:619,638` | "prepare"/"takeover" suggestions run a model-written prompt (built from calendar text and file names) in a fresh, untainted session. Off by default. | NEW | REPRO of the policy cell (`m6`); end-to-end TRACED | Start the session tainted, as `app.rs:223` does for triggered jobs (owner decision: changes prompting). |
| C35-8 / M-TG-1 | Telegram | `src/app.rs:139-152`, `src/permissions.rs:257,261`, `src/telegram.rs:207` | Phone turns inherit the desktop mode. In Auto mode a paired Telegram chat ran `shell` (`echo from-the-phone > pwned.txt`) with no question on the phone. Anyone who takes over the owner's Telegram account gets command execution. | NEW | REPRO (`telegram-auto-mode-shell-repro.rs`) | Owner decision: cap remote-channel turns at Ask; allow `ExternalWrite`/`ProcessExec` approval only at the computer. |
| O26-1 | PR #26 | `pr/26:src/sources.rs:213-229` → `agent.rs:285-290`, `app.rs:561` | Google/Microsoft calendar text reaches system prompts untainted (same root as M-3). | KNOWN root (T2) | TRACED | As M-3. Blocks #26. |
| O26-2 | PR #26 | `pr/26:src/oauth.rs:185-187,203-205`, `server.rs:1099-1100` | A revoked or expired consent can only be repaired by Disconnect, which deletes the index and learned memories; "Reconnect" always fails while connected. | NEW | TRACED | `needs_reauth` state that re-consents the same account and swaps the token without purging data. Blocks #26. |
| M-GOV-1 | Repository | GitHub settings | `main` has no branch protection or rulesets; required checks and reviews are not enforced. | KNOWN (G6) | API response 404 / `[]` | Repository admin action. |

### P2

| ID | Location | Finding | Tag | Verification |
|---|---|---|---|---|
| M-7 | `src/app.rs:394` | `run_parked` uses a clean tool context, so a `remember` parked from a tainted job is saved as confirmed. | NEW | TRACED |
| M-8 | `src/telegram.rs:270` | Forwarded third-party Telegram messages probably arrive as the owner's own words. | NEW | SUSPECTED |
| M-DEP-1 | `Cargo.lock` `libsqlite3-sys` 0.30.1 | RUSTSEC-2026-0459 (CVE-2026-119375, CVSS 6.8): bundled SQLite < 3.54 can return uninitialised memory when a database is opened with a crafted `-wal` sibling. Reachable through `heron restore` of an untrusted backup (`src/store.rs:590`). Fix: `rusqlite` 0.41. | NEW | `cargo audit` |
| M-TEST-1 | `src/ui/views.rs`, `tests/integration.rs` | Tests pass only in UTC with no `OPENROUTER_API_KEY` set; on other machines they hide the integration suite. | NEW | REPRO; **fixed in PR #39** |
| M-UI-1 | Slint theme | Selected sidebar item and selected mode chip render blue text on grey, barely legible in the headless renderer; not confirmed in a real window (report 06). | NEW | Snapshot inspection |
| M-REL-1 | `src/connectors.rs:99` | Non-blocking `flock` on the connector registry; on Linux a subprocess spawned concurrently can briefly hold a just-closed lock, so a registry change reports "busy in another HERON instance" spuriously. Seen as a hosted Linux test failure on PR #39 (`registry_is_atomic_private_and_never_contains_keys`). | NEW | Hosted CI failure; cause reasoned, not reproduced |
| C35-1 | `pr/35:src/server.rs:1017-1036` | A well-formed but wrong bot token replaces the working one and unpairs the phone without a liveness check. | NEW | TRACED |
| C35-2 | `pr/35:src/channel.rs:278,345,396-411` | No single-gateway guard: leases not bound to the claimant, DEFERRED transactions give `database is locked`, revocation reaches one process only. | NEW | Reviewer scratch tests (35 busy errors in 1,200 ops; stale ack overwrite) |
| C35-3 | `pr/35:src/channel.rs:417,358` | Retry exhaustion drops a reply silently and releases its later chunks out of order. | NEW | Reviewer scratch test |
| O26-3..7 | PR #26 | No stored account identity; unused `openid`/`email`/`User.Read` scopes; disconnect deletes the credential before the data; provenance merge breaks per-account erasure (probe-confirmed); granular-consent rejection untested; Windows credential blob limit (suspected). | NEW | see `review-pr26.md` |

### P3

PR #35: C35-4 (crash mid tool call leaves no "interrupted" notice), C35-5 (poll loop spins on a
persistent spool error), C35-6 (spool rows never pruned), C35-7 (envelope sender check compares a
value with itself). PR #26: O26-8 (vault I/O under a std mutex in async code, missing
`Cache-Control: no-store`, no audit entries, hard-coded `127.0.0.1` callback). UI: composer hint
overlaps the input border. Hygiene: 946+ rustfmt hunks and clippy `-D warnings` failures (D-7).

## 4. Root causes

Most P0/P1 items share two causes:

1. **Taint enters only through tool outputs marked untrusted** (`agent.rs:523`, `app.rs:223,262`).
   System-prompt context, local files, shell output, MCP callers, proactive prompts and phone
   turns all reach the model as if they were the user's own words. D-2 rests on that being true.
2. **The secret-read gate is a name denylist.** It misses links (fixed in #40), Heron's own home
   (fixed in #40), shell expansion syntax and many credential locations.

Both are policy decisions that change when users get asked. They are listed as owner decisions
in the final report rather than changed by this run.
