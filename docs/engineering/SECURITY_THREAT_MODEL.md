# Security threat model

Scope: Heron running on one person's computer, talking to a model provider they chose.

## What is protected

1. The user's files, inside and outside the working folder.
2. Secrets: model keys, mail passwords, anything in key folders or `.env` files.
3. Heron's own state: memories, tasks, reminders, automations, persona, skills.
4. What leaves the computer: nothing private should go anywhere the user did not choose.
5. The record of what happened (activity log).

## Who can act against it

| Actor | What they control | Main route |
|---|---|---|
| Author of outside content | A web page, an email, a document, a calendar entry, an MCP tool's output | Hidden instructions the model may follow (prompt injection) |
| A malicious or compromised MCP server | Its tool descriptions and outputs, and its own process | Rewriting descriptions after trust; exfiltration through arguments |
| Another local program | Requests to the local API; Heron's MCP server | Guessing the token; calling served tools |
| Someone who finds the Telegram bot | Messages to it | Guessing the pairing code |
| Someone who controls the paired Telegram account | Messages from the owner's own chat | In Auto or Yolo mode, tools run without a prompt (T12) |
| The model itself | Every tool call | Mistakes, or following an injection |

Out of scope: an attacker who already controls the user's account on the machine, and the model
provider (which sees whatever the conversation contains).

## The enforcement point

Every tool call, from every entry point, is classified into one `Effect` and decided by
`Policy::decide_in`. Entry points that reach it: the conversation loop, automation steps
(`Agent::call_tool`), approvals given later (`App::run_parked`), and tool calls from other apps
(`mcp::serve`). The decision order is fixed:

1. `Forbidden` is refused in every mode.
2. The user's deny rules beat every allow.
3. Read-only mode refuses everything that changes anything.
4. A read that may expose secrets always asks.
5. After outside content has entered the conversation, anything that changes state or sends
   model-composed text out asks, in every mode including unrestricted.
6. Only then: allow rules and the mode's conveniences.

The full table is the test `permissions::tests::the_decision_table`.

Outside content reaches the model only through tool results, which are wrapped and taint the
conversation. It is never copied into a system prompt: calendar entries in particular (ICS, Google,
Microsoft) are looked up with `upcoming_events` or `search_knowledge` rather than listed up front
(O26-1). The test is
`agent::tests::calendar_entries_stay_out_of_the_prompt_and_taint_the_turn_once_looked_up`.

## Web-fetch address and redirect boundary

HERON's `web_fetch` and `web_search` use an independent HTTP client, separate from any
external browser or model-provider HTTP calls. By default it ignores inherited
`HTTP_PROXY`, `HTTPS_PROXY`, and `ALL_PROXY` variables and resolves each origin
through `PublicOnlyResolver` at connection time. Initial and redirected requests
reject internal/literal hosts; redirects also reject credentials in the URL,
unexpected URL schemes, and HTTPS-to-HTTP downgrades. These checks reduce DNS
rebinding and redirect-based server-side request forgery (SSRF).

`HERON_ALLOW_LOCAL_FETCH` must explicitly equal `1`, `true`, or `yes` to
permit local targets; defining it as `0` or `false` does not enable it. This is
an intentional high-trust override and must not be used in ordinary operation.
`HERON_TRUST_WEB_PROXY=1` deliberately re-enables inherited system proxies.
**That is a trust transfer**: for proxied destinations the proxy resolves the
origin, so HERON cannot enforce its own connect-time origin-IP filter. Only
enable it with a proxy that reliably blocks internal/metadata destinations;
the synchronous hostname/IP checks and redirect host checks remain defense
in depth but are insufficient against DNS rebinding via a permissive proxy.

These are tool-specific transport controls, not a solution for T3 (shell
network egress) or T4 (browser subresource interception). Tests:
`web_fetch_exceptions_require_explicit_true_values`,
`redirect_cannot_escape_origin_safety_policy`, and `ssrf_guard`.

## Chat stream backpressure

Every event from every session, including background runs and Telegram turns, goes onto one
event bus (`App.events`, `src/events.rs`). Each event carries a `seq` that starts at 1 for each
core process. Clients read the bus through `GET /api/events?since=<seq>`, or through the
`POST /api/chat` SSE response, which is the view of one run's events.

The bus is bounded in two places, so a slow or stalled reader never makes the core allocate
without limit:

- **A ring of the last 10,000 events** (`RING_CAPACITY`) serves clients that reconnect with
  `since`. A `since` older than the ring holds, or newer than the last `seq` (after a core
  restart), gets `resync` at once.
- **A live channel of 1,024 events** (`LIVE_CAPACITY`) feeds connected readers. A reader that
  falls further behind gets `resync` and is dropped. The run carries on; nothing is aborted
  and no event is lost from the bus. After `resync` a client reloads with
  `GET /api/sessions/{id}` and `GET /api/approvals`, then continues from the newest `seq` it has.

A dropped or lagging `/api/chat` response ends only that response. Only `POST /api/cancel` stops
a run. Approvals wait on the bus, not on one stream: an approval nobody answers within the
approval timeout (900 seconds by default, `[server] approval_timeout_secs`) is denied and parked
(`park_approval`): the run carries on without that call, the bus gets `approval_parked`, and the
parked call can be approved later from the approvals list. Tests: `events::tests::lagging_subscriber_gets_resync_and_is_dropped`,
`events::tests::bus_resyncs_when_since_is_too_old`, `server::tests::run_survives_dropped_chat_stream`
and `server::tests::approval_waits_on_the_bus_then_parks_on_timeout`.

The desktop limits its network-to-UI event queue and pending native UI-thread
callbacks to **64 each**. Its incremental SSE parser rejects an individual
line larger than **4 MiB**, even across partial response chunks. This protects
against non-terminating lines and huge serialized messages. Both a broken
connection and parser failure are reported to the UI rather than leaving a
permanently busy conversation.

**Residual limits:** The ring and the live channel bound the *number* of
events, not their individual serialized byte lengths. Other application
channels and event transports require their own audits. Because a run no
longer stops when its reader goes away, a run started by a client that then
disappears keeps using the model until it ends or is cancelled; an approval
nobody answers is denied and parked after the timeout, and the run carries on.
The bounds stop a slow reader from growing memory, but do
not replace per-client rate limits or a complete denial-of-service analysis.

## Known weaknesses (not mitigated)

| # | Weakness | Consequence |
|---|---|---|
| T1 | Taint is per conversation, not per datum. | Coarse: one fetched page makes every later change ask. Safe, but noisy. |
| T2 | Facts learned from mail or documents are stored unconfirmed, yet can be recalled into a clean conversation without marking it as tainted. | An injected "fact" could steer a later conversation that looks clean. |
| T3 | On macOS the shell tool's network is limited to the egress allowlist (see Harness safety below), but the sandbox for CLI providers leaves the network open, and Linux has no network filter and Windows has no sandbox. Commands the agent runs through its shell tool no longer inherit tokens and keys from the environment, but can still read files the sandbox allows. This covers the shell tool only: the user's own `notify_command` and connected MCP server programs still start with the full environment. | An approved command can send what it can read anywhere. |
| T4 | The browser checks addresses before and after navigating, not each request. | A page can make the browser request internal addresses. |
| T5 | One browser and one "last seen" table for all conversations. | Element numbers from one conversation could be used in another. |
| T6 | A page listing many addresses can signal a few bits by which the user approves. | Low-bandwidth leak with user approval. |
| T7 | Command classification is pattern based. | A cleverly written command may be rated lower than it deserves; the sandbox is the real boundary where it exists. |
| T8 | The activity log is hash-chained but lives in the same database. | Someone who can rewrite the database can rewrite the whole chain. |
| T9 | The local API's own endpoints (the buttons in the app) are not subject to the permission mode. | By design they are the user's direct actions; a program that steals the launch token can use them. |
| T10 | Installers are unsigned. | A user cannot verify who built them. |
| T12 | Telegram turns run in the desktop's permission mode. Accepted by the owner ([C35-8](../ACCEPTED_RISKS.md#c35-8-phone-turns-inherit-the-desktop-permission-mode)). | In Auto or Yolo mode, whoever controls the paired Telegram account can run tools, including `shell`, with no prompt. Ask mode still asks for every change, and high-risk or outside-content requests are approved only at the computer. |
| T11 | Undo replaces by atomic exchange and verifies what was displaced (D-13). Removing a file still has only the narrowed check-then-remove gap: the check is repeated at the last moment, after the old version is ready on disk, but the check and the removal are still two steps. | A save from another program that lands in the instant between the removal check and the removal is deleted without a copy. No operating-system primitive closes that one. |

## Harness safety

Source: [the harness-safety design](../designs/heron-harness-safety.md). It adds an effect journal, a loop
guard, protected paths, an egress allowlist and a toolchain check around the agent's shell. Each entry
below is one row of that design's Risks table: what could go wrong, what stops or detects it, and what
stays open. None is an accepted-risk entry, because `ACCEPTED_RISKS.md` is capped at 3. They sit next
to Harness2's "model-written code on the Mac" entry (HQ2), which relies on this sandbox.

Owner-only settings: the egress list, the "never" lifts and the toolchain re-trust are changed only
through Heron's authenticated API, from Settings › Sandbox. No agent tool can change them, because the
file tools are refused `config.toml` and `heron.db` and the shell can't reach them.

#### HS-R1. Uploads to allowed hosts that accept writes

*   **Risk:** `github.com` and `registry.npmjs.org` are on the default egress list, and both accept
    authenticated writes (`git push`, `npm publish`). A command holding an attacker's token could send
    data out through them. The proxy does not decrypt TLS, so it can't tell a fetch from a push.
*   **Source:** OQ5.
*   **Mitigation:** `publish` commands are `Privileged` and ask every time, with no standing rule. In a
    tainted conversation every mutating shell call asks. `crates.io`, the host `cargo publish` uses, is
    off the list.
*   **Left open:** an upload the owner approves, and a `git push` over HTTPS with a token the command
    itself holds.

#### HS-R2. Domain fronting through a CDN-hosted allowed host

*   **Risk:** an allowed name that sits behind a CDN can be used to reach a different site on the same CDN.
*   **Source:** §3.2.
*   **Mitigation:** the proxy compares the TLS ClientHello's server name with the CONNECT host and
    refuses a mismatch.
*   **Left open:** the HTTP `Host` header inside the TLS session is not checked, so a CDN that routes on
    it can still reach another site.

#### HS-R3. A local forwarding proxy

*   **Risk:** a process that forwards traffic onward (an HTTP debugger, `ssh -D`, a local model proxy
    on `127.0.0.1:8080`) bypasses the allowlist, because loopback stays open to the shell. It stays open
    because Heron's own tests and Harness2's `cargo test --locked` bind and connect on loopback.
*   **Source:** OQ4.
*   **Mitigation:** `agent.deny_local_ports` lists ports the shell can't connect to. Heron's own API
    ports are denied as well, although its token, not the port deny, is what protects the API.
*   **Left open:** any forwarder on a port that is not in the list.

#### HS-R4. A sandboxed command reaching a daemon through a system service

*   **Risk:** a command could ask a daemon that holds the owner's keys or network access (Docker, an
    SSH agent, a password manager) to act for it, through a socket or a mach service.
*   **Source:** PT-E1.
*   **Mitigation:** the local-socket rules from #79, the egress rules' denies for known daemon
    sockets and the resolver, and PT-E1's service tests.
*   **Left open:** a mach service that PT-E1 did not try.

#### HS-R5. A replaced toolchain run by the owner's own tools

*   **Risk:** the shell may write to the Rust toolchains, so it could replace `rustc` or `cargo`. The
    owner's next unsandboxed build would then run the replacement.
*   **Source:** OQ1.
*   **Mitigation:** `toolchain::check` before and after each sandboxed command, with a notice when a
    binary changed, and a read deny so Heron's own commands don't use a changed toolchain.
*   **Left open:** the time until the owner reads the notice. OQ1's stronger option, which removes the
    write allowance, closes it, and the owner has not chosen it.

#### HS-R6. Edited crate sources in the Cargo caches

*   **Risk:** the shell may write to `~/.cargo/registry/src` and `~/.cargo/git/checkouts`, so a crate's
    source can be edited there.
*   **Source:** OQ2.
*   **Mitigation:** none.
*   **Left open:** the edited source runs in the owner's next unsandboxed build.

#### HS-R7. Build inputs in the working folder

*   **Risk:** `build.rs`, `.cargo/config.toml` and `package.json` scripts run on the owner's next build.
*   **Source:** the design's Non-Goals.
*   **Mitigation:** none. Writing those files is the agent's normal work.
*   **Left open:** they run in the owner's next build.

#### HS-R8. Hard links to a protected file

*   **Risk:** a link made inside the working folder to a file the profile denies for writing could
    give a write path to it.
*   **Source:** PT-P1.
*   **Mitigation:** Seatbelt refuses a hard link to a write-denied file. The rehash is defence in depth.
*   **Left open:** a link made outside the sandbox before the run, and any change between runs, which the
    rehash doesn't see.

#### HS-R9. The config of a repository made during the run

*   **Risk:** a repository created during the run has a config that can name hooks or a filter command.
    Seatbelt can't deny it, because the scan that builds the deny list ran before the repository existed.
*   **Source:** OQ7 (e).
*   **Mitigation:** the rehash at the end of the run. The repository's hooks are denied at any depth.
*   **Left open:** the change is made before it is reported.

#### HS-R10. A protected file changed between two runs

*   **Risk:** a protected file is edited while no run is active, for example by another program.
*   **Source:** §3.3.
*   **Mitigation:** none. The hash check compares a run's start with its end.
*   **Left open:** not reported.

#### HS-R11. A command running at the same time as the one that changed the toolchain

*   **Risk:** two shell commands overlap, and one replaces a toolchain binary while the other uses it.
*   **Source:** G7.
*   **Mitigation:** the next toolchain check sees the change.
*   **Left open:** the overlapping command uses the changed toolchain.

#### HS-R12. Heron's API reached from the sandbox

*   **Risk:** loopback is open to the shell, so a command can try Heron's local API.
*   **Source:** OQ4.
*   **Mitigation:** every `/api` route needs the token, and the token is kept in memory and never written
    where the sandbox can read it. The profile also denies Heron's own ports.
*   **Left open:** a server in another process that fell back to a random port. It still needs the token.

#### HS-R13. A CLI provider that runs its own shell

*   **Risk:** under `cli_allow_agent_tools`, `cline-cli` and `goose-cli` run their own tool loop and shell
    inside the CLI sandbox.
*   **Source:** the design's Non-Goals.
*   **Mitigation:** the Cli profile lets the CLI write only its own run folder (`src/sandbox.rs:402`).
    Without `cli_allow_agent_tools` set, Heron refuses to run these two.
*   **Left open:** its network is open (`docs/cli-providers.md`, Containment), so the egress allowlist
    does not apply to it.

#### HS-R14. Power loss between a tool's effect and its intent commit

*   **Risk:** the machine loses power after a tool changed something and before the journal row for it
    was committed, so the journal doesn't know the effect happened.
*   **Source:** §1.3.
*   **Mitigation:** `synchronous=FULL` on the journal's database.
*   **Left open:** macOS `fsync` is not `F_FULLFSYNC`, so the drive can still hold the write in its cache.

#### HS-R15. Orphaned children of an interrupted shell command

*   **Risk:** Heron dies while a shell command runs. The command's children keep running, and a re-run
    could start a second copy.
*   **Source:** §1.4.
*   **Mitigation:** the ask waits while the recorded process group is alive, for up to one hour. The
    gate for repeats of the same call covers the call in the meantime.
*   **Left open:** a child that leaves its process group, or one still running after an hour.

#### HS-R16. Protected files changed in a run that Heron's shutdown cut short

*   **Risk:** Heron shuts down while a run is active, so the end-of-run comparison of protected files
    doesn't happen.
*   **Source:** §1.2.
*   **Mitigation:** the skipped check is recorded in the audit log and the inbox.
*   **Left open:** the change is not compared, so the owner must check by hand.

## Accepted risks

The owner-signed register is [ACCEPTED_RISKS.md](../ACCEPTED_RISKS.md). Each risk there links
back to its entry here.

- **C35-8**: phone turns inherit the desktop permission mode. It is recorded as T12 in the table
  above, which links to
  [its register entry](../ACCEPTED_RISKS.md#c35-8-phone-turns-inherit-the-desktop-permission-mode).
- **R-1**: working-folder reads stay trusted, so injected text in a working-folder file can steer
  the model to send other working-folder content out without a prompt; reads outside the working
  folder and secret-looking paths still ask, which breaks the M-1 credential chain.
  [Register entry](../ACCEPTED_RISKS.md#r-1-working-folder-content-can-be-sent-out-after-an-injected-read).

## Keeping this current

Any change to `src/permissions.rs`, a tool's `risk` function, or a new entry point that can run a
tool must update this file, the decision table test, and REMEDIATION_MATRIX.md in the same change.
