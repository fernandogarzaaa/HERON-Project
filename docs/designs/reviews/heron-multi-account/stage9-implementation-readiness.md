# Stage 9: implementation readiness (resolved)

Status: resolved

**Target:** `docs/designs/heron-multi-account.md`, the Detailed Implementation Plan in draft `be8b3dc`.

**Reviewers** (Goldfish, each given only the doc, the provider facts, the frozen PR #72 tree and `Cargo.toml`/`ci.yml`):
*   `egm-implementation-readiness`: 2 Blockers, 12 Majors, 11 Nits
*   `egm-codebase-details`: 2 Blockers, 12 Majors, 9 Nits
*   `egm-style`: 11 Majors, 16 Nits

The plan section was rewritten in full, and the architecture was patched where the plan needed it.

## Blockers (both reviewers found the same two independently)

| Finding | Change |
|---|---|
| Verified vault reads always fail on Linux (`config.rs:936-938`), which breaks Email and Calendar learning and an existing test on the ubuntu CI runner. | On a platform with no vault, and a home not enrolled in the test vault, `vault_only_secret_checked` returns `Ok(None)`. Each caller's handling of a read error is defined: learn skips OAuth, `sync_due` skips OAuth, `GET /api/oauth` returns 503, and everything else returns the error. New test `a_checked_read_of_a_missing_entry_is_none`. The existing `learning_from_email_reports_items_as_waiting` must still pass unchanged. |
| A retried migration's "newest copy" backup replaced the true pre-upgrade copy. | A `before-multi-account` copy is made only when none exists, and it is never pruned. New test `the_first_migration_backup_survives_a_retry`. |
| U1's check passed when required tests were `#[ignore]`d. | The check now also lists ignored tests and fails if a required name is among them. A constraint now bars OS-limited and ignored required tests. |

## Majors

| Finding | Change |
|---|---|
| The plan named a cancel route that §5 and Q-4 reject. | Removed. |
| The Swift model had `oldItems`, and nothing kept the two sides' JSON in step. | `oldItems` removed and `readOnly`/`connected` added. The §9 literals are now the contract: U2 decodes them exactly, and U1's i7 and sign-in tests assert their key sets. The meaningless fixture line is dropped. |
| There was no server test constructor, Host had no port, and the callback couldn't be reached. | Added `#[cfg(test)] router_with_http`, a Host with a port, `send_raw` for non-`/api` paths, and test-vault enrolment in the harness. |
| `any_connected` and `upgrade_pending` returned `bool`. | Both now return `Result<bool>` (`any_listed`, `upgrade_pending`), and read errors are returned. |
| "Connected" was defined in several places. | `AccountRecord::connected()` defines it once, and `needs_sign_in` is `!connected()`. |
| The near-match exclusion was ambiguous and covered only one candidate loop. | It now applies by account (`account_of_source` on both sides) on all three paths, and bare rows stay candidates. |
| Old entries were read with an unverified `secret()`, so one Keychain error could misfile the account permanently. | `legacy_entry` reads each old field with a verified read, plus a new `fallback_file_secret`. Any error stops the run. New test `migration_stops_on_an_unreadable_old_subject`. |
| Vault error text could reach API responses through `{:#}`. | The error message is fixed, and the OS text goes to the log only. |
| `rename_source` per base contradicted "one transaction". | Replaced with `rename_sources(&[(from, to)])` in one transaction. |
| Many specified behaviours had no named test. | Named and gated: the prompt and hint table, the client-ID order, the unknown `account_id` 404, the 17th-record cap, disconnect failing waiting sign-ins, provider-level routes with zero, one and two accounts, the `learn_one` bare refusal, the `disconnect_source` rules, and the status fields. |
| The converted tests had no names. | Now `a_failed_record_write_keeps_the_old_entries` and `migration_files_a_fallback_file_token_as_signed_out`. |
| The testteeth gate was self-contradictory and had the wrong base. | The base is the `multi-account-base` tag set before `skein init`. The gate is the score alone, and equivalent mutants are listed in the PR. |
| The old OAuth API had no stated fate. | The deletions are listed, and clippy finds stragglers. `secret_key` is renamed `legacy_key` for `legacy_entry` only. |
| `ProviderHttp` didn't reach `get_json` or the fetchers. | Those signatures are listed, and `FakeProvider` is module-level and keyed by refresh token and (access token, URL). |
| The check at `server.rs:1593` was ill-defined. | It now reads "any listed account of either provider", and read errors are returned. |
| Swift disconnect dispatch would send per-account rows to `disconnectSource`. | Added `enum PendingDisconnect`, with its dispatch and its forget-sentence form (§10). |
| The fate of existing Rust and Swift tests was unstated. | Each existing test is listed as kept, changed, renamed or deleted. The vault-live test keeps its exact name for CI's `--exact` run. Preview controls get replacements. |

## Nits applied

*   **Errors and limits.**
    *   An unknown provider stays HTTP 400, as today.
    *   A malformed body is refused by the extractor.
    *   The cap rises from 8 to 16, with the reason given.
*   **Code details.**
    *   `SYNC_LOCKS` uses `LazyLock`.
    *   The design reuses `random_secret()` and `new_token()`.
    *   New store tests go in `store::tests`.
    *   The logging claim is corrected.
    *   Enrolled homes take the verified delete path on every OS.
*   **Behaviour.**
    *   An unreadable record is a distinct error, so the migration never overwrites it.
    *   The client-ID order uses the most recent listed account that has a client ID.
    *   A renewal keeps the old label when the new token has none.
    *   A callback with no `code` and no `error`, or a provider mismatch, ends as failed.
*   **Swift.**
    *   `accountID` is mapped through transfer structs.
    *   `syncOAuth` is dropped.
    *   Disconnect returns counts.
    *   An expired record during the first sync ends the spinner.
    *   Swift tests use the existing `testCamelCase` style, and the check greps the exact names.
*   **Plan and checks.**
    *   `verify` gains the desktop `--bins` build.
    *   The sandbox reason is stated, with the source of the 27-test figure.
    *   The open fix PRs are named.
*   **Wording.** "Node" became "unit", the seven migration steps are listed in item 1, test names replace invariant IDs, and the account-ID labels are consistent.

## Dismissed

*   **Style N8, renaming "suffixed" to "per-account source name" everywhere.** Applied in the plan. A few uses of "suffixed" remain in architecture prose where they are unambiguous.
*   **Readiness "not verified" items.** I computed the test vectors with `shasum`. They match §1.

## Re-audit (the one allowed)

One `egm-implementation-readiness` reviewer checked the fixes above against the doc text. It confirmed 2 of the 3 Blockers and most Majors fixed. It found 1 Blocker and 5 Majors still open, partly-resolved items and new ones together. All were fixed. The re-audit cap is now used up, so no further audit ran.

| # | Sev | Finding | Change |
|---|---|---|---|
| A | Blocker | `start_oauth` returns 501 off macOS and Windows, so the server sign-in tests would fail on Linux CI. | Under `#[cfg(test)]`, an enrolled test home passes the `start_oauth` OS check (`server.rs:1379-1382`). Release builds keep the 501. |
| B | Major | An empty or cut-short backup file would count as "already backed up". | "Usable copy" now means it opens read-only and passes `damage` (`store.rs:279`). A bad file is ignored, and the test gains an empty-file case. |
| C | Major | An unreadable record had no rule outside `accounts`. | Every caller treats it as "no record" (§2, item 1). |
| D | Major | The legacy-source refusal changed with no test, and §3 and item 6 disagreed. | §3 now says "either provider". New gated test `disconnect_source_refuses_legacy_email_while_a_signed_out_account_is_listed`. |
| E | Major | Swift `account(_:)` was redefined against its callers. | It is replaced by `SettingsStore.provider(_:)`, with today's fallback, and `accounts(_:)`. Callers are listed. |
| F | Major | The renamed live-vault test was never run by any check. | U1 requires its name in the full test list, and `verify` runs both live-vault tests on the owner's Mac. |
| G | Major | The bundled client ID (`option_env!`) couldn't be controlled in tests. | New `oauth::bundled_client_id`, with a test override for enrolled homes. `start_oauth` and `client_configured` use it. |
