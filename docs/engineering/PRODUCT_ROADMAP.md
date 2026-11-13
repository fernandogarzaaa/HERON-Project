# Product roadmap

Order follows the mission's phases. Each entry names the first shippable slice, because every
one of these is larger than a single change. Status lives in REMEDIATION_MATRIX.md.

## Phase 1 — safety (in progress)

- Done: effect model, read-only for all state, outside-content rule in every mode, search no
  longer exempt, MCP server under the same policy.
- Done since: allowlisted environment for commands.
- Next: treat recalled unconfirmed facts as outside content (T2); per-conversation browser state;
  request-level checks in the browser.

## Phase 2 — durability

- Done: approvals given later are claimed atomically; undo checks the file has not changed
  since and writes are atomic; due jobs are claimed in one transaction and interrupted runs are
  reported.
- Next, in order: retry with backoff and per-job pause for jobs; `user_version` migrations with a
  snapshot before upgrade; backup and restore.

## Phase 3 — platform security

- Windows: either a verified boundary (job object + restricted token) or no autonomous commands there.
- MCP: per-tool grants and call budgets.
- API: bounded queues, body-size limits, token not carried in the address bar.

## Phase 4 — quality

- Model capability table (tools, streaming, context, reasoning tokens) replacing the fixed
  2048-token floor and the wording-based router.
- Local diagnostics: per-request spans, p50/p95, failure categories, JSON export.
- Memory: cache hash vectors, benchmark 1K/10K/100K, expand the evaluation set.

## Phase 5 — product

- Trust centre: one page listing every grant (standing and per conversation) with revoke.
- Automation editor over the existing step schema, with a dry run that cannot have side effects.
- Explainable memory: why a fact was recalled and from where.
- OAuth connectors, voice and screenshots: each optional, each behind the same policy. OAuth
  needs provider app registrations before any live test.

## Phase 6 — delivery

- Format and lint the tree, then add the gates. Add `cargo audit`/`cargo deny`.
- Fuzz and mutation testing for the policy and parsers.
- Signing and notarisation once identities exist; SBOM and provenance in the release workflow.

## Not planned

- Any mandatory cloud service, account or telemetry.
- Automatic updates that run a downloaded program without the user's confirmation.
