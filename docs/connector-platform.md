# HERON Connector Hub, Credential Vault and Proactivity — Architecture

**Status: connector foundation plus reviewed Remote MCP runtime. REST execution remains disabled.** A catalog entry is never evidence that HERON can authenticate to that service.

## Research and interoperability

Claude has a managed connector directory and supports custom remote MCP endpoints. ChatGPT provides an app/plugin directory and MCP-based custom integrations with permissions. Grok supports built-in OAuth, a connector catalog and custom MCP. Muse uses connected apps, scoped approvals, proactive suggestions and a credential store that the agent cannot inspect.

Sources:
- https://support.claude.com/en/articles/11176164-use-connectors-to-extend-claude-s-capabilities
- https://help.openai.com/en/articles/11487775-connected-apps-in-chatgpt
- https://developers.openai.com/plugins/quickstart
- https://docs.x.ai/grok/connectors
- https://ai.meta.com/muse/

MCP permits interoperability with compatible SERVER protocols and authentication flows. It does not transfer a user's Claude, ChatGPT or Grok access tokens, proprietary connector implementation or paid subscriptions to HERON. Each user must authorize HERON separately.

## Architecture

1. **Directory / discovery.** Built-in catalog, configured local stdio MCP entries and custom endpoint registrations. Catalog candidates do not imply working service integrations. Custom MCP states distinguish inactive registration, tested/awaiting review and enabled scoped access.
2. **Connection and credential broker.** One anonymous or native-vault bearer identity per registration. Use distinct registrations for separate accounts. Keys are injected only at the registered HTTPS endpoint and never returned to the model. Google/Microsoft OAuth remains separate draft PR #26; Linux rejects new connector-key storage.
3. **Runtime adapters.** Remote Streamable HTTP MCP implements modern per-request metadata and supported legacy initialization, bounded discovery/streaming, schema validation and non-replaying invocation. REST/OpenAPI execution and OAuth-only remote endpoints remain unsupported.
4. **Execution policy.** Process-local tool grants bind one reviewed connection test, credential identity and registry revision, expire after one hour/restart, and are invalidated by tool changes or revocation. Every invocation requires an operation approval; model tools cannot enable connectors. Results are untrusted.
5. **Proactivity.** Existing #32 protections still refuse unattended MCP execution. Connector event subscriptions, per-account polling and event provenance remain separate work; Takeover does not acquire external write authority.

## Implemented local API

All routes require HERON's existing per-launch API bearer token. Mutation routes additionally require the authenticated loopback-only server.

| Route | Operation |
| --- | --- |
| GET /api/connectors | Catalog, existing stdio MCP entries, custom metadata and vault backend status; NO secrets |
| POST /api/connectors/custom | Store validated custom public HTTPS endpoint metadata, without activation |
| DELETE /api/connectors/custom/{id} | Remove registration and its credential |
| POST /api/connectors/custom/{id}/api-key | Store a user-supplied API key in the verified OS credential vault only |
| DELETE /api/connectors/custom/{id}/api-key | Delete that key and invalidate grants |
| POST /api/connectors/custom/{id}/remote/test | Discover and review MCP capabilities; no tool execution |
| GET /api/connectors/custom/{id}/remote | Inspect the tested manifest and current grant state |
| POST /api/connectors/custom/{id}/remote/grant | Enable explicit reviewed tool scopes for one hour |
| POST /api/connectors/custom/{id}/remote/call | Invoke one granted tool with fresh operation approval |
| DELETE /api/connectors/custom/{id}/remote | Revoke access and cancel local in-flight requests |

Example registration body:
{"id":"my_company","name":"Company API","category":"productivity","transport":"rest_api","endpoint":"https://api.example.org/v1","permission":"read_only"}

No HTTP request is sent on registration. Endpoint metadata requires HTTPS with a public DNS hostname, no embedded userinfo, query, fragments or nonstandard port. The remote MCP transport additionally enforces connect-time public DNS checks and refuses redirects; URL registration validation alone is not an SSRF defense.

**Security limit:** The existing Config.store_secret uses macOS Keychain / Windows Credential Manager when working, but falls back to an owner-only plaintext file, and Linux has only the plaintext fallback. This is NOT acceptable for a broad login vault. The new connector API therefore requires successful verified native OS credential storage and fails closed on Linux. A future Linux Secret Service or encrypted/passphrase-protected vault with recovery is a required gate. Do not store raw website passwords, cookies, banking logins or payment details in connector metadata or HERON config. Use OAuth, passkeys, or a scoped password-manager broker instead.

## Product catalog and rollout

Discovery areas: Communications, Files, Productivity, Developer, Social, CRM, Commerce and Personal/Home. Candidates include Google, Microsoft, GitHub, Drive, OneDrive, Dropbox, Notion, Slack, Linear, Discord, Instagram, Facebook, YouTube, TikTok, X, Salesforce, HubSpot, Shopify, Stripe and Home Assistant. Every service needs its own authorized API, provider scopes, account isolation, data retention and limits.

**Phase 1:** remote MCP test/review/grant/invoke/revoke is implemented and mock-tested. Qualify exact-head CI/security review and live compatible endpoints; Google/Microsoft PKCE and OAuth sign-in remain separate.
**Phase 2:** custom REST/OpenAPI broker with operation allowlists, opaque vault handles, scoped credential injection only into approved provider requests, audited egress, transport retries, timeouts, rate limits, and read/write separation.
**Phase 3:** official social/commerce integrations, previews before posting, no CAPTCHA or MFA bypass, no silent high-impact actions, provider reconciliation and rollback where supported.

## Pro Activity improvement plan

Existing HERON Awareness has Off, Suggest, Prepare and Takeover modes, file-name observations, a background token budget, quiet periods, and accept/dismiss suggestion controls. It lacks per-connector event provenance and per-account proactive permissions.

The next PR should implement a ConnectorEvent envelope recording connector ID, account ID, provider event ID/hash, source reference, observation timestamp, sensitivity, TTL and consent version. No credential payload in the event journal. Deduplicate events and enforce event polling and notification budgets. Show users why a suggestion was generated or suppressed, and which source produced it.

Data from email, docs, social and websites is **untrusted**. Never obey instructions within external content. Connector-connected background jobs can gather and prepare under read-only grants, but write actions to external services must queue explicit human approval even in Takeover mode. Pause, disconnect, expired consent and revoked permissions must cancel or invalidate in-flight work. Track useful accepted suggestions, unwanted interruptions, denied operations, staleness, spend and latency, with deterministic/adversarial evals before enabling additional autonomous permissions.

## Release qualification

- Full Rust test suite, desktop build and exact-SHA CI on Linux/macOS/Windows.
- Manifest schema, duplicate ID, untrusted URL, corrupted registry and private file-mode tests.
- Keychain storage/retrieval/deletion with disposable credentials on macOS and Windows; Linux must reject key storage until a secure backend exists.
- Registrations never grant invocation. MCP execution requires explicit reviewed scopes and per-operation approval; REST execution remains disabled.
- Live provider consent and revocation tests for every connector before labeling it Available.

## Foundation qualification follow-up

Credential writes and revocation now use verified transactions with deterministic
fault injection, redacted failures and a consistent uncertain-state recovery
warning. Registry reads are bounded before allocation. Cross-process tests exercise
contention and owner termination; authenticated API tests prove registration stays
inactive. See [qualification evidence and remaining production gates](engineering/CONNECTOR_QUALIFICATION.md).


## Remote MCP implementation

The remote runtime adds explicit test, capability review, scoped enablement,
per-operation approval and revocation for compatible public HTTPS MCP servers.
Registration still grants nothing. See [runtime, API, compatibility and operating
instructions](remote-mcp-runtime.md). OAuth PR #26 and connector-aware events are
separate. The runtime document records protocol limitations and the exact local qualification scope.
