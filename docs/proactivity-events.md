# Source-backed Pro Activity — P0 event and consent journal

**Status:** non-executing foundation. No connector polling, live webhook, remote credential, social-platform post, or new autonomous tool permission is activated by this PR. The current HERON awareness modes remain unchanged.

## Background

HERON already has local file-awareness and suggestion workflows. Adding provider integrations requires a separate trusted evidence pipeline so an email, social post, issue comment, or shared document cannot pretend to be user instructions or trigger silent actions. Issue [#34](https://github.com/fernandogarzaaa/HERON-Project/issues/34) covers multi-channel routing and the broader harness.

## Implemented data contract

`src/proactivity_events.rs` defines `ConnectorEvent` with:
- Connector and account IDs, opaque upstream event ID, hashed evidence reference, consent version, observation timestamp, TTL, and sensitivity.
- Consent registry per connector/account, with version and expiration. Events lacking active current consent are rejected.
- Deduplication of provider event IDs per connector/account; event payload is deliberately **not** stored.
- A per-account five-suggestion-per-day budget, restricted/sensitive event suppression, and an explicit pending status for future human review.
- Revocation purges the account's events and suggestions; expired events can be pruned.
- SQLite journal with local-only owner permissions on Unix, secure deletion and bounded event count.

This is an evidence/control interface, **not** a recommendation engine or autonomous executor. Future adapters must verify provider-signed ingress, map account identifiers to confirmed local grants and pass only opaque evidence references. Do not accept consent versions from untrusted message content.

## Security boundaries

- Content from email, social networks, shared files, and websites is untrusted. It can provide evidence but never instructions for privileged actions.
- Consent is evaluated per account and version. A revoked connector must stop background work, cancel in-flight operations, and remove any cached credentials through the existing vault/adapter lifecycle.
- Notifications and external writes remain behind HERON's regular permission gate; new inbound data cannot silently enable `Takeover` actions.
- No raw access tokens, email bodies or social messages should appear in the event journal. The underlying local database is not encrypted; encrypt disk/backups if required.
- This first PR is intentionally *not* integrated into the existing `proactive_tick` loop. Ingest activation requires qualified provider connectors and UI permission controls.

## Qualification

- `cargo test --locked --lib proactivity_events::tests`
- `cargo test --locked`
- `cargo build --locked --features desktop --bins`
- Green exact-head CI on Linux, macOS and Windows, plus independent security review.
- Dedicated tests for stale consent, revocation, source isolation, duplicate provider IDs, TTL expiry, budget exhaustion and sensitivity suppression.
- Follow-up end-to-end tests with a verified provider test account, rotating tokens, backfilled/stale events, disconnect while processing, replay and API rate-limit errors.

## Next work

1. Connect qualified provider adapters to this journal, with canonical event IDs, signature verification and account-specific scope grants.
2. Build a visible Slint activity timeline displaying source, reason, privacy classification, whether a proposal was suppressed, and a user-editable notification budget.
3. Tie proposed actions to the existing approval API. Execute nothing merely because an event arrived; use deterministic and adversarial evals for high-impact tools.
4. Add provider reconciliation, explicit retry state, and notification delivery to the channel gateway, after both PRs are independently qualified.
