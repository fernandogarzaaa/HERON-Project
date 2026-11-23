# Stage 6: comprehension review (resolved)

Status: resolved

**Target:** `docs/designs/heron-multi-account.md` (draft `586b677`)
**Reviewer:** `egm-comprehension` (Goldfish; given only the doc, the provider facts and the frozen PR #72 tree)
**Result:** 4 Blockers, 14 Majors and the Nits below. All are fixed except one partial Nit.

| # | Severity | Finding | Decision | What changed |
|---|---|---|---|---|
| B1 | Blocker | The "needs sign-in" flag lived in SQLite, but the functions that touch it only get `cfg`. | FIX | The flag is now the per-account secret-store key `oauth_<p>_<id>_needs_sign_in` (§2). Signatures stay `&Config`. |
| B2 | Blocker | Nothing let tests fake the vault or the provider. | FIX | New "Test seams" section: a test vault keyed by home (and how it passes both OS checks), `ProviderHttp::Fake` with paused calls, no backup for in-memory stores, and how the two existing tests change. |
| B3 | Blocker | U1 couldn't pass its check without U2's server work. | FIX | U1 and U2 are merged into one Rust unit. The Mac app and the guide are now U2 and U3. |
| B4 | Blocker | Admission didn't cover a listed account with no stored subject. | FIX | The admission table now has five explicit cases, plus the `Admission` and `SignInError` types. Listed with no subject gives `Refreshed` and stores the subject. |
| M1 | Major | The client-ID fallback was gone once the old key went away. | FIX | §5 gives the client-ID order for Add and for Sign in again. Start takes `account_id` instead of a hint. §5 also gives the `client_configured` rule. |
| M2 | Major | The sign-in record lifecycle was unclear. | FIX | §5 now gives states, retention from completion, a cap of 16 with the existing 429, single use per `state`, poll locking and Cancel. |
| M3 | Major | Wire shapes were incomplete. | FIX | §9 adds JSON examples, null handling, error statuses and `expires_in_seconds: 600`. Provider-level sync behaviour on partial failure is stated. |
| M4 | Major | It was unclear who runs the first sync. | FIX | The callback owns it. The record's `first_sync` ends the Mac spinner, and the Mac app no longer calls sync. |
| M5 | Major | `learn_one` with a bare OAuth name had no rule. | FIX | It is refused with "Unknown source", and `learn` leaves it out. |
| M6 | Major | `finish` error pass-through was ambiguous. | FIX | A closed `SignInError` list is used, everything else gets the generic text, and only the top-level message is shown. |
| M7 | Major | A vault delete failure during Disconnect had no rule. | FIX | It returns the error, the account stays listed, and Disconnect can be retried. |
| M8 | Major | The state after a failed migration was undefined. | FIX | Providers are independent. The API returns `upgrade_pending`, Settings shows a notice, Add still works, and the zero-account DELETE keeps today's behaviour. |
| M9 | Major | An old connection with no epoch or no client ID wasn't handled. | FIX | Step 3 writes a fresh epoch. A missing client ID falls back to the bundled ID, or sets "needs sign-in". |
| M10 | Major | The async migration held a std mutex. | FIX | The migration is a plain `fn`, holds `SOURCE_WRITES` per provider, and never awaits. |
| M11 | Major | The `invalid_grant` message named Google only. | FIX | The message uses the provider's name. The rule is HTTP 400 plus `error == invalid_grant` for both providers. |
| M12 | Major | Single-source search on an OAuth base wasn't covered. | FIX | A single OAuth base uses the family search. |
| M13 | Major | The family API was described differently in two sections. | FIX | New family functions are defined in item 8. The exact-match functions stay. `upcoming` merges per base and truncates. |
| M14 | Major | Cancel and Slint sign-ins could add accounts quietly. | FIX | A cancel route was added. The Slint app's behaviour is documented as accepted (multi-account Slint UI is a non-goal). |
| N | Nit | Undefined terms. | FIX | A Terms list was added under the Overview. |
| N | Nit | Wrong citations. | FIX | `sync_due` is `:577`. The migration hook is "just before `App::assemble` returns". |
| N | Nit | The account ID encoding and test vectors were missing. | FIX | The encoding is stated and two real vectors are given. |
| N | Nit | No label fallback. | FIX | Rows show "‹Provider› account", and Sign in again with no label uses the chooser prompt. |
| N | Nit | Memory tags kept the old names. | FIX | Step 5 renames tags that equal the bare base. |
| N | Nit | The step 5 wording, and downgrade then re-upgrade. | FIX | The wording now says the already-renamed row is kept. Downgrade without restore is documented, and its worst case is "Sign in again". |
| N | Nit | Step 6 wasn't a step. | FIX | Removed. The steps are renumbered, and the commit point is now step 6. |
| N | Nit | Registry parsing, lock map growth and two backups. | FIX | All three are specified. There is one backup per start. |
| N | Nit | The success page text. | FIX | It reads "Account connected" or "Account renewed". |
| N | Nit | The grep check assumption. | FIX | Stated: exactly one use in `src/`, inside `is_outside`. |
| N | Nit | `login_hint` validation. | PARTIAL | The hint is now always the stored label, which `account_label` already validated (length 254 or less, no control characters). Clients no longer send a hint. |
