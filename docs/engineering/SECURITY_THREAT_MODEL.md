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

HERON's native and web chat clients consume the same `POST /api/chat` SSE endpoint.
The server caps each pending chat event queue at **128 events**. EventSink is
synchronous; when a stalled consumer fills that queue, the active run is
aborted instead of silently dropping model/tool/approval events or allocating
unbounded memory. A best-effort terminal error is sent once capacity returns
(with a five-second timeout), otherwise the stream ends early and the desktop
and web clients show a failed/interrupted turn. When the SSE response is
dropped, its guard aborts only its original run id and removes outstanding
approvals; a newer reply in that conversation cannot be cancelled by stale
cleanup. Approval prompts use bounded, timed delivery and fail closed if the
client cannot receive them.

The desktop limits its network-to-UI event queue and pending native UI-thread
callbacks to **64 each**. Its incremental SSE parser rejects an individual
line larger than **4 MiB**, even across partial response chunks. This protects
against non-terminating lines and huge serialized messages. Both a broken
connection and parser failure are reported to the UI rather than leaving a
permanently busy conversation.

**Residual limits:** The server channel bounds the *number* of events, not
their individual serialized byte lengths. Other application channels and
event transports require their own audits. A sufficiently slow client can
still hold one reply or approval until its timeout; the guard prevents it
from accumulating unlimited queued events, but does not replace per-client
rate limits or a complete denial-of-service analysis.

## Known weaknesses (not mitigated)

| # | Weakness | Consequence |
|---|---|---|
| T1 | Taint is per conversation, not per datum. | Coarse: one fetched page makes every later change ask. Safe, but noisy. |
| T2 | Facts learned from mail or documents are stored unconfirmed, yet can be recalled into a clean conversation without marking it as tainted. | An injected "fact" could steer a later conversation that looks clean. |
| T3 | The OS sandbox for commands leaves the network open, and there is none on Windows. Commands the agent runs through its shell tool no longer inherit tokens and keys from the environment, but can still read files the sandbox allows. This covers the shell tool only: the user's own `notify_command` and connected MCP server programs still start with the full environment. | An approved command can send what it can read anywhere. |
| T4 | The browser checks addresses before and after navigating, not each request. | A page can make the browser request internal addresses. |
| T5 | One browser and one "last seen" table for all conversations. | Element numbers from one conversation could be used in another. |
| T6 | A page listing many addresses can signal a few bits by which the user approves. | Low-bandwidth leak with user approval. |
| T7 | Command classification is pattern based. | A cleverly written command may be rated lower than it deserves; the sandbox is the real boundary where it exists. |
| T8 | The activity log is hash-chained but lives in the same database. | Someone who can rewrite the database can rewrite the whole chain. |
| T9 | The local API's own endpoints (the buttons in the app) are not subject to the permission mode. | By design they are the user's direct actions; a program that steals the launch token can use them. |
| T10 | Installers are unsigned. | A user cannot verify who built them. |
| T12 | Telegram turns run in the desktop's permission mode. Accepted by the owner (C35-8). | In Auto or Yolo mode, whoever controls the paired Telegram account can run tools, including `shell`, with no prompt. Ask mode still asks for every change, and high-risk or outside-content requests are approved only at the computer. |
| T11 | Undo replaces by atomic exchange and verifies what was displaced (D-13). Removing a file still has only the narrowed check-then-remove gap: the check is repeated at the last moment, after the old version is ready on disk, but the check and the removal are still two steps. | A save from another program that lands in the instant between the removal check and the removal is deleted without a copy. No operating-system primitive closes that one. |

## Keeping this current

Any change to `src/permissions.rs`, a tool's `risk` function, or a new entry point that can run a
tool must update this file, the decision table test, and REMEDIATION_MATRIX.md in the same change.
