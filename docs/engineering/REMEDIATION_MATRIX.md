# Remediation matrix

Status of every item in the engineering mission, as of the head of branch
`security/effect-model` (stacked on PR #2). Audited `main`: `8ecd2fa3e78f36a734eadda913ed68244002b242`.

Status words, used strictly:

- **Verified**: implemented, with a named automated test that fails without the change.
- **Partial**: some of the item is implemented and verified; the rest is listed.
- **Not started**: nothing in the code yet.
- **Blocked**: cannot be done from this environment; what is needed is stated.

No item is "done" on the strength of a document.

## A. Permission system

| Item | Status | Evidence / what is missing |
|---|---|---|
| A1 Typed effect model replaces `Risk::Safe` | **Verified** | `permissions::Effect` (10 effects). Tests: `the_decision_table`, `every_built_in_tool_declares_what_it_changes` (every built-in tool, with read-only denial of every mutation), `policy_modes`. |
| A1 Read-only denies all state changes at every entry point | **Partial** | Agent, automation steps and approvals-given-later share `Policy::decide_in`. MCP server now does too (`another_app_cannot_change_anything_in_read_only_mode`). **Missing:** the local HTTP API's own endpoints (add memory, add task, add reminder from the UI) are the user's direct actions and are not gated by mode; see DECISIONS D-3. |
| A2 Outside-content rule holds in `Yolo` | **Verified** | `promises_that_hold_whatever_the_settings` (600 combinations), `outside_content_restricts_what_happens_next`. |
| A2 Provenance records with source IDs and causal edges | **Partial** | Per-conversation taint flag (persisted) plus a set of user-authored addresses rebuilt from the user's own messages. **Missing:** per-datum provenance, source IDs, propagation through compaction summaries (compaction currently fails closed: addresses in dropped messages stop counting). |
| A2 URL from untrusted output is not user-authorized | **Verified** | Only addresses in the user's own text count; `after_outside_content_nothing_outbound_or_lasting_happens_unasked`, `outside_blocks_are_removed_whole`. |
| A3 `web_search` exemption removed; egress check on all model-composed transmissions | **Partial** | Search, fetch, browser open/click/type, MCP arguments are classified `NetworkEgress`/`ExternalWrite` and asked about after outside content in every mode. **Missing:** typed destination/payload/data-class policy; subprocess network traffic (the OS sandbox leaves the network open); Telegram replies. |
| A3 Narrow grants for legitimate research | **Verified** | "Allow searches for this conversation" (`may_grant_for_conversation`); fetching a model-built address is never covered. |
| A4 Invariant tests | **Partial** | Exhaustive enumeration over modes × effects × taint × rule sets. **Missing:** a property-testing/fuzz harness, concurrency permutations, malicious email/MCP/skill fixtures (web-page fixture exists). |

## B. Isolation

| Item | Status | Evidence / what is missing |
|---|---|---|
| B1 Browser request interception, redirect/subresource checks | **Not started** | Only pre-navigation and post-navigation checks exist. Known gap: a request can happen before the post-check. |
| B1 Per-session browser state | **Not started** | One global browser and one global `last_seen` table. Known cross-session hazard. |
| B1 Screenshots behind opt-in | **Not started** | |
| B2 Allowlisted subprocess environment | **Verified** | Commands start from an empty environment and receive a fixed list (paths, language, toolchain locations, certificate bundles) plus names the user adds in `shell_env`. Proxy settings pass only without a password. The variable named as holding the model's key is withheld even when listed. Test: `commands_see_a_short_list_of_variables_not_the_whole_environment`. **Not covered:** the `notify_command` and MCP server processes still inherit the full environment. |
| B2 Resource limits, process-tree cancel, output ceiling | **Partial** | Output ceiling and process-group kill exist (`endless_output_is_cut_off_instead_of_filling_memory`). No memory/CPU limit. |
| B2 Windows containment | **Not started** | No sandbox on Windows; autonomous shell is still allowed there. This is an accepted, documented risk for the beta, not a pass. |
| B3 MCP trust tiers, per-tool grants, budgets | **Partial** | Trusted servers are now `NetworkEgress`, unknown ones `ExternalWrite`; schema changes force approval. No per-tool grants, budgets or environment containment. |
| B4 API hardening review | **Not started** | Existing: loopback, token, Host/Origin checks, CSP. Not re-reviewed; channels are unbounded. |
| B5 Atomic settings writes, migrations, backup/restore, key rotation | **Partial** | Secrets file is atomic and serialised (`a_damaged_secrets_file_is_never_overwritten`). `config.toml` write is not atomic; no versioned migrations, backup or rotation. |

## C. Durability

| Item | Status | Evidence / what is missing |
|---|---|---|
| C1 Atomic claim of approvals given later | **Verified** | `Store::claim_approval` (`new → running` in one statement). Test in `app::tests`: 20 concurrent approvals execute once; an interrupted one becomes `unknown`, is not repeated. **Missing:** lease/heartbeat, revalidation of tool fingerprint and taint at execution. |
| C2 Undo with hash preconditions and conflict detection | **Verified** | Each checkpoint records a hash of what Heron wrote. Undo refuses when the file has since been edited, replaced or removed; the checkpoint is kept until a restore succeeds; a forced restore first saves the current version beside the file, under a name created exclusively (an earlier saved copy is never replaced) and with the file's own permissions. Writes and restores are atomic (temporary file, then rename), and the temporary file is created no more readable than the file it replaces. Tests: `undo_restores_only_what_is_still_as_heron_left_it`, `a_write_that_fails_leaves_nothing_to_undo_and_no_stray_files`, `forced_undo_never_replaces_an_earlier_saved_copy_or_loosens_who_can_read_it`. **Missing:** fault injection for a crash between checkpoint and write (the order makes it safe: a checkpoint without a hash restores as before); symlink-swap race test. |
| C3 Durable run queue, leases, retries, DST policy, pause/resume | **Partial** | A due job is claimed in one transaction that moves the schedule on and writes a `running` record, only if it is still due as seen (`Store::claim_due_job`). A run cut short by a restart is marked `unknown`, reported in the inbox and not repeated. Test: `a_due_job_is_claimed_once_recorded_and_never_silently_lost`. **Missing:** heartbeat/lease expiry (single process is assumed), retry with backoff, per-job pause/resume, concurrency cap, per-job budget, explicit time-zone and misfire policy, event-triggered jobs (they are not claimed this way). |
| C4 Resumable workflow steps, save-time and run-time validation | **Partial** | Validation at save and run exists; no resumable state. |
| C5 `user_version` migrations, pre-upgrade snapshot, integrity tests | **Not started** | Schema changes are additive `ALTER TABLE` checks. |

## D. Model and runtime

| Item | Status | Evidence / what is missing |
|---|---|---|
| D1 Capability-aware router with provider adapters | **Not started** | PR #2 has a two-model heuristic router (`usage::route`), not capability metadata. |
| D2 Per-task model choice, policies, private-data boundary | **Not started** | |
| D2 No hard-coded token minimum | **Not started** | PR #2 sets a 2048-token floor for every provider. Flagged by the mission; to be made per-model. |
| D3 Structured-output validation, cassette conformance | **Partial** | Cassette record/replay exists; no structured-output retry or provider conformance suite. |
| D4 Local observability (spans, p50/p95, export) | **Partial** | Token ledger per day and model, Usage page, audit log. No spans, latency percentiles or diagnostics export. |

## E. Memory

| Item | Status | Evidence / what is missing |
|---|---|---|
| E1 Cached hash vectors, incremental reindex, ANN benchmarks | **Not started** | Hash vectors are recomputed per neural query. |
| E2 Memory provenance fields, per-source unlink | **Partial** | Source, confirmation state and supersession chain exist. No quote/locator, expiry or extraction-model field. |
| E3 Explainable memory UI, contradiction queue | **Not started** | |
| E4 Expanded evaluation | **Not started** | Only the 34-question set (unchanged results). |

## F. Product

| Item | Status | Evidence / what is missing |
|---|---|---|
| F1 Trust and Action Control Center | **Partial** | Approval cards with reasons per effect, "Waiting for you", activity log, conversation-wide grants. No page to review and revoke grants, no emergency-stop control beyond Stop and Pause. |
| F2 Automation Studio | **Not started** | |
| F3 OAuth connectors (Gmail, Microsoft 365) | **Not started** | Also **Blocked** for live verification: needs provider app registrations and user-owned accounts. |
| F4 Voice | **Not started** | |
| F5 Screenshot-assisted browsing | **Not started** | |
| F6 Split `assets/index.html` | **Not started** | |

## G. Delivery

| Item | Status | Evidence / what is missing |
|---|---|---|
| G1 fmt / clippy / audit / deny gates | **Not started** | Measured baseline: `cargo fmt --check` reports 946 diff hunks (the code has never been rustfmt-formatted); `cargo clippy --all-targets` reports 39 warnings (one of substance: a lock held across an await); `cargo audit` and `cargo deny` are not installed. Adding the gates before fixing these would only produce a red or masked check. |
| G2 Mutation testing | **Not started** | |
| G3 Fuzz targets | **Not started** | |
| G4 Installer qualification, signing, SBOM, provenance | **Blocked** | Needs signing identities (Apple Developer ID, Windows code-signing certificate) and real machines. The v0.1.0 installers are unsigned and have only been run on Linux. |
| G4 Stray `index.html` in releases | **Verified (workflow)** / **Blocked (existing release)** | Fixed in the workflow (`aa25965`, merged). The file is still attached to v0.1.0; this session cannot edit releases. The owner must delete it. |
| G5 Signed update checking | **Not started** | |
| G6 Branch protection | **Blocked** | Needs repository admin. Steps are in QUALIFICATION.md. |
| G7 Developer documentation | **Partial** | This folder. No setup scripts. |
