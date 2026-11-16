# Connector foundation qualification — 2026-11-16

## Repository evidence

Inspection started from main `d854755159feacda891f9731b3b949765bece241`.
All remote branches and PR head refs were fetched without resetting existing work.
PR #32 is merged as `a7ce6d0` and #33 supplies bounded chat/SSE backpressure.
The only open PRs observed on GitHub are draft #31 and draft #26.

Foundation PR #31 was inspected at
`b2ba00faf334da27899c99fd879a03824b0ee9bc`. Its public conversation reports
resolved earlier review findings and a clean review on that SHA. Its public
checks page shows six successful jobs in run `43659782894` (Linux/macOS/Windows
standard and neural jobs). Those results apply to that old head, not the
changes in this qualification branch. Main was merged normally into this branch;
no OAuth commits were imported and no shared history was rewritten.

OAuth PR #26 remains at `f4d715d80c662e550af03cea8cd1131ef7d1fe3e`.
Its own documentation explicitly says it is unqualified and stores refresh tokens
through the legacy plaintext fallback on Linux. It is not a production-ready
credential broker and has not been merged here.

GitHub HTTPS Git reads work through existing platform authentication. At initial
inspection, `gh api repos/fernandogarzaaa/HERON-Project/pulls/31` returned Forbidden.
Public HTML supplied PR descriptions, comments, open-PR membership and job status.
A draft network addition for `api.github.com` was saved; it does not apply that
policy to the current machine or establish API write access.

## Implemented corrections

* Credential replacement now uses one injectable transaction boundary. It reads
  the previous state before mutation, verifies the new value, then cleans up any
  legacy copy. Every failed write, read-back or cleanup restores and verifies the
  previous value or verifies absence. Failed rollback always produces the same
  manual-revocation warning consumed by native Settings.
* Credential revocation verifies absence, and an OS entry-construction failure is
  an error rather than false success. Raw backend errors are discarded at the
  connector transaction boundary, including their error chains.
* Registry reads stop at 256 KiB plus one byte before parsing, rather than
  allocating the entire file and checking its size afterwards. Names reject
  control characters, and lock-file open semantics explicitly preserve contents.
* Deterministic child-process tests prove lock contention fails closed, retains
  the first registration, accepts the second after release, and releases the
  OS lock after owner termination. These use the Rust test executable directly
  and run on Unix and Windows without shell-specific scripts.
* A socket-level API test proves bearer authentication, Host/Origin restrictions,
  non-loopback mutation denial, inactive registration, absence from model tools,
  audit redaction, Linux vault refusal and removal despite legacy-file corruption.

## Reusable components and gaps

| Component | Reuse | Required follow-up |
| --- | --- | --- |
| MCP | Existing stdio JSON-RPC, discovery/calls, SHA-256 manifest pins, untrusted output | Remote Streamable HTTP, current-spec negotiation, schema validation, bounded streaming, cancellation and revocable grants |
| Permissions | Effect classification, tainted conversation rules, explicit approvals | Per-account tool/scope grants and broker validation at dispatch |
| Vault | Profile namespaces, native macOS/Windows keyring, verified transactions | Linux Secret Service/encrypted unlockable store; OS-only model keys; multiple accounts; refresh/expiry/revocation broker |
| Slint Settings | Catalog and inactive endpoint CRUD, write-only key entry | Search/categories, connection test, tool review, account management, focus/accessibility qualification |
| Scheduler | Off/Suggest/Prepare/Takeover, budgets, approval queue; #32 blocks unattended MCP | Consented event envelopes, deduplication, TTL, account-generation fencing and disconnect cancellation |
| HTTP/Tests | Local bearer token, Host/Origin validation, mock model/API/MCP fixtures, headless renderer | Remote malicious-server contracts and live provider qualification |

## Product requirements and technical blueprint

The next implementation must preserve this state progression:
`registered -> tested -> tools reviewed -> scoped account authorized -> enabled`.
Registration never grants execution. Changed capabilities invalidate review;
revocation fences both queued work and in-flight result commits.

A trusted broker, outside model-visible tools, owns opaque per-account vault
handles. It binds credentials to the reviewed HTTPS destination, account,
operation and scopes. It validates the grant again immediately before dispatch;
redirects cannot carry credentials to a new destination. Network transport must
pin validated public DNS results at connection time and reject mixed public/private
answers, redirects to internal addresses, oversized responses and unsupported
protocol versions. Do not reuse the general web-fetch proxy override as a
credentialed connector security policy.

Implement remote MCP against an inspected official specification/SDK version,
including the requested July 2026 changes only after verifying that specification.
Do not advertise compatibility with an uninspected future/versioned protocol.
All schema references must remain local; schema validation must not fetch arbitrary
URLs. Never retry a possibly executed tool call automatically without an
idempotency contract. Tool output and provider events remain untrusted data.

An event record requires connector/account IDs, provider event ID, consent
version, observation time, TTL, sensitivity and source reference. Deduplicate
before budgeting; fence work on account generation and consent at consumption
and commit. External writes, purchases and destructive actions need explicit
operation approval even in Takeover mode. Revoking consent invalidates pending
suggestions and tasks, rather than silently reauthorizing them on reconnect.

## Compatibility matrix

| Connection | Evidence / status |
| --- | --- |
| Existing local stdio MCP | Mock-server integration exercises real process I/O; not a universal server certification |
| Custom HTTPS MCP and REST entries | Metadata registration only; no connection, authentication or invocation |
| macOS Keychain / Windows Credential Manager | Production adapter and injected transaction tests; new-head OS execution and disposable live credential tests required |
| Linux connector credentials | Unsupported; fail closed with no plaintext fallback |
| Legacy IMAP/ICS | Existing implementation; no live account qualification in this task |
| Google / Microsoft OAuth | Separate draft #26; not imported, not live tested, not production qualified |
| Catalog-only services | Planned/candidate entries, never operational claims |
| Optional neural memory | Existing feature; not a connector and not requalified locally |

## Operating and release gates

Use the existing checkout and the cloud environment's saved installation steps.
Initialize `. /workspace/.heron-cloud/env.sh`, then work in
`/workspace/HERON-Project`. No external accounts or API keys are needed for the
foundation tests. The local child-reaping wrapper compensates for this container's
PID 1 leaving zombies; it does not alter assertions or filter tests.

```sh
python3 /workspace/.heron-cloud/reap-run.py cargo test --locked
cargo clippy --locked --all-targets
cargo fmt --all -- --check
cargo build --locked --features desktop --bins
HERON_TEST_BROWSER=1 python3 /workspace/.heron-cloud/reap-run.py cargo test --locked --features desktop --bin heron-desktop --test integration
bash tests/linux-install-smoke.sh
```

The Chromium command needs the platform-supported elevated shell execution in
this cloud environment. Do not disable Chromium sandboxing to make it pass.
Repository-wide formatting has existing drift (for example `examples/mock_llm.rs`)
and is not silently fixed as part of this security change. Clippy warnings are
reported separately from compilation/test failures.

Do not merge the qualification changes based on CI for `b2ba00f`. Require tests,
Slint compilation and fresh security review on the resulting exact commit, with
Linux/macOS/Windows jobs and disposable native vault round trips on supported OSes.
Run `cargo test --locked --lib config::tests::connector_vault_live_round_trip -- --ignored --exact` on each authorized macOS/Windows desktop; this test is not executed on Linux. The standard macOS/Windows CI jobs now invoke it explicitly as a required step; a locked or unavailable native backend must fail qualification. Use non-force pushes. API permission/network denial and unavailable OS/account
authorization are external blockers, not passing checks.

After the foundation gate, deliver remote runtime, OAuth/broker, native connection
management, event intelligence, then production hardening as separate reviewable
changes. Google qualification needs a real public desktop OAuth client, enabled
Gmail/Calendar APIs and a consenting test account. Microsoft needs an Entra public
desktop registration with the documented loopback redirect and consenting account.
Use `docs/oauth-integrations.md` from #26 for that draft's exact redirect contract.
Do not request tokens, client secrets or user passwords in chat. Verify granted
scopes, refresh rotation, disconnect erasure and reconnect account isolation with
live applications before promoting either integration.

Production readiness is **not established** by the foundation changes. Remote
execution, Linux vault, OAuth live tests, multi-account grants, consent UI and
connector-aware proactivity remain required implementation/qualification work.

### Local results

On Debian 13 x86_64 with Rust 1.100.0:

* Full suite: 223 library, 1 CLI, 9 integration and 22 setup tests passed;
  doctests contained zero tests and are not counted as functional coverage.
* Desktop suite: 31 tests passed, including the headless window renderer;
  the feature-enabled integration suite passed all 9 tests with real Chromium.
* `cargo build --locked --features desktop --bins` passed.
* `cargo clippy --locked --all-targets` completed successfully with warnings
  (including existing unused imports, field-reassignment and dead-code warnings).
* `git diff --check` passed. The new transaction module passes rustfmt.
  Repository-wide rustfmt check failed on existing style drift; no broad reformat
  was applied. Linux installer smoke was already verified during environment
  onboarding; no packaging scripts or dependencies changed in this follow-up.
* Native macOS/Windows vault qualification and cross-platform CI for these new
  changes remain unrun locally. Prior-head CI is not substituted for either.

Fault tests include failures before mutation, partially successful writes,
read-back errors, legacy cleanup errors, both forms of failed rollback, a lying
delete backend, and unreadable post-delete state. Removing the verification,
OS lock, authentication or inactive-registration checks breaks these assertions.


### Follow-up CI gate

Run `43794035178` completed with all six jobs successful for `e6a47ab`.
That run did not execute the ignored native-vault round trip. CI now explicitly
runs it on macOS and Windows, without `continue-on-error`, using only disposable
profile-isolated credentials. Results must be checked on the new commit before
claiming native vault qualification. The latest visible security review still
covers `b2ba00f`, not the qualification changes. GitHub API access remains denied;
no fresh review or merge has been performed.
