# Remote MCP runtime

Status: implementation and deterministic local qualification. This is not a live
certification of any catalog service. Google/Microsoft OAuth PR #26 is separate.

## Protocol sources and implementation choice

The official specification inspected is revision **2026-07-28**, repository commit
`2be673cfa63064b243a6016588411343c0f9fdee`:

- [Streamable HTTP](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/2be673cfa63064b243a6016588411343c0f9fdee/docs/specification/2026-07-28/basic/transports/streamable-http.mdx)
- [Version negotiation](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/2be673cfa63064b243a6016588411343c0f9fdee/docs/specification/2026-07-28/basic/versioning.mdx)
- [Discovery](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/2be673cfa63064b243a6016588411343c0f9fdee/docs/specification/2026-07-28/server/discover.mdx)

The official Rust SDK, `rmcp` 3.10.0, was inspected at
`0e2396f73a88617141b31f645de6f1171ed5f993`, including its protocol models and
Streamable HTTP client. HERON uses a small explicit HTTP binding on its existing
reqwest 0.12 stack to keep DNS policy, response bounds, dispatch and non-replay
behavior visible at the broker boundary. The SDK's current HTTP adapter uses
reqwest 0.13.2. No unpinned Git SDK dependency or proprietary transport is used.
JSON Schema evaluation uses `jsonschema` 0.33 with default network/file retrieval
features disabled, plus explicit reference/complexity restrictions.

Modern requests carry protocol version, client identity and empty capabilities
in `params._meta`; HTTP headers mirror version, method, tool name and valid
`x-mcp-header` parameters. Unsafe ASCII/Unicode/sentinel values use the specified
base64 sentinel encoding. Modern complete results require `resultType=complete`.
The client does not send modern initialize/initialized, GET or DELETE sessions,
Last-Event-ID, unsolicited client responses, or sampling/elicitation requests.

The read-only `server/discover` probe recognizes modern version errors. A safe
legacy fallback performs initialize/initialized and uses the returned session ID
for Streamable HTTP 2025-11-25, 2025-06-18 or 2025-03-26. Deprecated 2024 HTTP+SSE,
modern MRTR input requests and subscription streams are not implemented or
advertised. Tool capabilities are refreshed before every invocation instead of
relying on a long-lived change stream. A change notification encountered during
an SSE response invalidates access. Some servers requiring unsupported client
capabilities will correctly fail rather than receive invented responses.

## Lifecycle and consent

Native Settings → Connector Hub:

1. Register a public HTTPS MCP endpoint. No network request or grant results.
2. Optionally save its bearer API key in the supported OS vault. An anonymous
   connection needs no key. Use a distinct connector ID for each account.
3. Enter the registered connector ID and choose **Test and review capabilities**.
   The test may send that registration's key to its exact HTTPS endpoint; it only
   discovers capabilities and cannot call tools.
4. Review the untrusted manifest, including schemas/descriptions. Enter an explicit
   comma-separated set of tool names and approve those scopes for one hour.
5. Enter a tool and JSON arguments, then **Approve and invoke once**. This is an
   explicit approval for that operation, not a standing permission for writes.
6. Revoke access, remove the key or remove the registration to stop local requests
   and invalidate grants. The underlying provider key is not remotely revoked by
   disabling local access; delete/revoke it with the provider if required.

Each review token binds one connection test, not just a reusable hash of identical
tool definitions. Grants are process-local and lost on restart/retest. The agent
uses stable `mcp__remote__catalog` and `mcp__remote__call` dispatchers; only enabled
registrations appear in the catalog. It cannot test, approve, enable or extend a
grant through these tools. Calls use `SensitiveWrite`, forcing a new approval in
Ask, Auto and Yolo and respecting read-only/deny policies. Existing untrusted-output
handling and the #32 unattended `mcp__*` deny rule remain in force.

A registration marked `read_only` can be tested but cannot receive invocation
scopes: third-party effect annotations are not trusted proof that a tool is safe.
Use `approval_required` for MCP execution; a fresh operation approval is still mandatory.

The broker dispatcher names are reserved after configured stdio tools attach.
A third-party server named `remote` cannot shadow `mcp__remote__call` and bypass
its approval/revocation boundary; collisions are reported in MCP status.

No authentication inherited from Claude, ChatGPT, browser sessions or OAuth #26
is used. OAuth-only remote servers are unsupported until the separate broker work.
Linux still refuses new connector keys; anonymous remote MCP remains usable there.

## Local authenticated API

All routes require the existing per-launch bearer token. These remote admin
routes additionally require a loopback-bound server and valid Host/Origin.

| Method and path | Body / behavior |
| --- | --- |
| POST `/api/connectors/custom` | Existing metadata registration; use `transport: remote_mcp` |
| POST `/api/connectors/custom/{id}/remote/test` | Discover, validate and return capabilities plus a review token; disable prior grants |
| GET `/api/connectors/custom/{id}/remote` | Current tested manifest and grant status; no credentials |
| POST `/api/connectors/custom/{id}/remote/grant` | `{"fingerprint":"<review-token>","tools":["tool_name"]}` |
| POST `/api/connectors/custom/{id}/remote/call` | `{"tool":"tool_name","arguments":{},"approve_once":true}`; ordinary read-only/deny policy still applies |
| DELETE `/api/connectors/custom/{id}/remote` | Cancel local work and persist a revision change that fences other instances |

Failures have a structured `error.code` and static `error.message`; raw HTTP,
provider, schema-instance and OS-vault errors are withheld. Audit records use the
connector ID and fixed outcomes, never credential values, tool arguments or raw
provider output. A failed call is never transparently replayed. A timeout may
mean the provider executed the operation: reconcile before approving another call.

## Security and concurrency

- HTTPS public DNS hostnames only. Every new direct connection validates the actual
  DNS answer set; any private, special-use or mixed public/private result fails.
  DNS results used for connecting are the validated results. HTTP/1 is used and
  idle connection pooling is disabled. TLS certificate verification stays enabled.
- No redirects, system proxies, environment-controlled localhost overrides, cookies
  or ambient authentication. Credentials are injected only as a sensitive bearer
  header at the exact registered endpoint. Proxy-only networks may be unsupported;
  failure must not be worked around by disabling address screening.
- Broker reads use only the native connector vault, never the legacy plaintext
  fallback. Grants bind both metadata revision and a private credential digest;
  credential replacement, even directly in the OS vault, requires retesting.
  Digests are not returned through the API or catalog.
- Registry revisions fence independent HERON processes. Registry changes and key
  mutations conservatively invalidate all old revision-bound grants. Local revoke
  cancels in-flight futures; dropping an invocation future also invalidates its
  grant through a drop guard. Cleanup is generation-conditional so an older
  cancelled call cannot tear down a newly approved connection; another process detects persisted revocation before
  dispatch and before accepting results. There is no cross-process interrupt
  service: a remote action already dispatched may finish, but stale results cannot
  restore grants or be accepted after revocation is observed.
- Limits: 32 runtime entries, 128 tools, 16 discovery pages, 512 KiB aggregate
  manifest, 64 KiB per schema, 2,048 expanded schema/argument nodes, bounded depth,
  64 KiB arguments/structured outputs, 1 MiB HTTP responses, 128 KiB SSE frames,
  128 data events / 512 total frames including keepalives, 2 calls per registration and at least 100 ms between starts.
  Connection timeout is 8 seconds, HTTP timeout 25 seconds and whole operation
  deadline 30 seconds. Repeated pagination cursors fail closed.
- Only acyclic local JSON Pointer schema references are supported. External,
  recursive and dynamic references are rejected. Invalid header annotations fail
  the connection rather than enable a partially understood manifest. This is
  deliberately stricter than the specification's option to exclude individual
  malformed tools. Invalid output/schema or transport/protocol failure invalidates
  local grants. No arbitrary resources are fetched by the schema validator.
- Capability definitions reflecting the current raw credential are rejected.
  Tool results are recursively redacted before leaving the broker, including
  JSON-escaped strings and object keys. As with any authorized service, this does
  not establish that the service itself is trustworthy or cannot transform data.

## Reproducible qualification

Initialize the saved cloud toolchain with `. /workspace/.heron-cloud/env.sh` and
run from `/workspace/HERON-Project`:

```sh
cargo test --locked --lib remote_mcp
cargo test --locked --lib remote_mcp::tests::authenticated_local_api_demonstrates_registration_review_approval_call_and_revoke -- --exact --nocapture
HERON_TEST_BROWSER=1 python3 /workspace/.heron-cloud/reap-run.py cargo test --locked --features desktop
cargo build --locked --features desktop --bins
cargo clippy --locked --all-targets --features desktop
rustfmt --edition 2021 --check src/remote_mcp/*.rs
cargo fmt --all -- --check
bash tests/linux-install-smoke.sh
```

The full cloud suite needs elevated execution for Chromium and the subreaper
wrapper for this container's orphan-process behavior. Neither changes assertions.
The deterministic server fixture uses HTTP loopback through a private `cfg(test)`
transport injection; production has no corresponding URL/proxy bypass. The tests
exercise real sockets and production protocol/broker/API logic, but do not qualify
public DNS, a live provider account, or the system browser's entire native consent
flow. Native controls compile and use standard labeled widgets; dedicated manual
keyboard/accessibility and live service qualification remain release work.

Required negative cases include invalid IDs/auth/origins, missing/stale grants,
changed manifests, schemas/references, duplicate tools, pagination loops, redirects,
private/mixed DNS answers, malformed JSON/SSE/envelopes, wrong response IDs,
output schema failure, credential reflection, revocation during discovery/call,
replaced scopes, expiry, concurrency, throttling and timeouts without replay.
CI runs standard and neural jobs on Linux/macOS/Windows for remote-MCP branches;
exact-head checks and a fresh security review remain prerequisites to merging.

## Compatibility matrix

| Server / mode | Status |
| --- | --- |
| Modern 2026-07-28, JSON and request-scoped SSE, anonymous/API key | Deterministic socket/API tests; no live provider certification |
| Legacy Streamable HTTP 2025-11-25 with session | Deterministic handshake and call tests |
| Legacy 2025-06-18 / 2025-03-26 | Deterministic version-error, initialization/session and call contract tests |
| Deprecated HTTP+SSE 2024-11-05 | Unsupported; register a Streamable HTTP endpoint |
| MRTR sampling/elicitation, subscriptions, resumable streams | Unsupported; not advertised |
| OAuth-only servers / multiple OAuth accounts | Unsupported pending separate authorization broker |
| Linux bearer-key storage | Unsupported; anonymous connections work without a key |
| REST/OpenAPI custom registration | Metadata only; remote MCP dispatcher refuses it |
| Google/Microsoft/other catalog services | No new live integration or subscription authorization implied |

## Local qualification results

On Debian 13 x86_64 / Rust 1.100.0, the final full desktop-enabled suite passed:
255 library tests (including the remote contract tests and subprocess fixtures),
1 CLI test, 31 desktop tests including software-rendered layout, 9 integration
tests with real Chromium, and 22 setup tests. Doctests contained zero tests and
are not counted as functional coverage. The focused remote suite passed 32 tests,
one of which is a child-process fixture rather than standalone coverage.

The pre-dispatch input-schema guard was deliberately removed in a temporary local
mutation: `input_schema_prevents_network_calls` failed because a network request
escaped validation. Restoring the guard passed the focused suite. No weakened
implementation or assertion is retained. Additional tests assert the generation,
revocation, namespace and scope boundaries directly.

One earlier broad run failed its existing connector-deletion assertion without
capturing the response body. The test now reports status/body on failure. The
isolated test, a running local API reproduction, ten independent diagnostic runs,
and subsequent complete suites passed. No root cause was established for that
single observation; exact-head CI and review remain mandatory, and it is not
reported as a confirmed fixed application defect.

The desktop build, Clippy and Linux installer smoke passed. Clippy retains existing
warnings. The new module passes rustfmt and the diff passes whitespace checks;
repository-wide `cargo fmt --all -- --check` still fails on existing style drift,
starting at unchanged `examples/mock_llm.rs`. This work does not perform a broad
reformat to hide that result. Test/build logs can be reproduced with the commands
above; local copies are kept in ignored `target/remote-mcp-evidence`.

Public-provider testing, manual native keyboard/accessibility qualification,
exact-head cross-platform CI and a fresh security review are separate gates.
There is no production-readiness or universal connector-compatibility claim.
