# Stage 7: critic review (resolved)

Status: resolved

**Target:** `docs/designs/heron-multi-account.md`, lines 1-655 of draft `a52e41f`.
**Reviewers** (Goldfish, each given only the doc, the provider facts and the frozen PR #72 tree):
*   `egm-design`: 1 Blocker, 10 Majors, 5 Nits
*   `egm-structural`: 4 Majors, 15 Nits
*   `egm-style`: 13 Majors, 17 Nits

The fixes were applied as one rewrite. The main change: one vault-only record per account replaces one entry per field, and the stage 5 contest on finding #13 is reversed. Alternative G records why.

## Design critic

| # | Sev | Finding | Decision | Change |
|---|---|---|---|---|
| 1 | Blocker | An account disconnected while the upgrade was incomplete came back at the next start. | FIX | §8 now defines "upgrade incomplete" whenever the old token is still present. In that state the migration target can't sync or learn. Disconnecting the target, or the provider-level delete, also removes the old token, the old entries and the bare rows. Test `i5_disconnect_during_an_incomplete_upgrade_stays_disconnected`. |
| 2 | Major | `secret()` returns `None` on any keyring error and `store_secret` can fall back to the file, so the registry could be wiped or tokens overwritten. | FIX | Each account is now one vault-only record. Every read is verified (`vault_only_secret_checked`), and a read error aborts. Writes are verified with rollback. Test `i3_a_vault_read_error_never_empties_the_registry`. |
| 3 | Major | Memory dedupe let account A update or retire account B's memories. | FIX | For a per-account OAuth save, rows from other per-account OAuth sources are left out of near-match candidates. G-2 now says this. Invariant I-9 and its test added. |
| 4 | Major | Keeping the epoch on renewal, with disconnect clearing it last, let partial disconnects bring data back. | FIX | Every admission, renewal included, writes a new epoch. Disconnect now deletes the record first, as one verified delete, and every step's error is raised. |
| 5 | Major | A stale `invalid_grant` could sign out an account that had just been renewed. | FIX | The flag is set under `SOURCE_WRITES`, and only if the rejected token and the epoch are still current. Test `a_stale_invalid_grant_never_signs_out_a_renewed_account`. |
| 6 | Major | Cancel during the token exchange still added the account. | FIX | Added an `exchanging` state. Cancel returns the current status, and the app keeps polling. Test `cancel_during_the_exchange_reports_the_result`. |
| 7 | Major | A half-finished Added left a live token that nothing listed. | FIX | The registry entry is written first, then the record. A failure leaves a signed-out row with Disconnect. |
| 8 | Major | Migration backups piled up and kept mail. | FIX | One `before-multi-account` backup per start, pruned to one copy. The README and the risk table say Disconnect doesn't purge backups. |
| 9 | Major | A restored pre-upgrade backup left bare rows nobody could erase, and a downgrade left tokens in the vault. | FIX | §8 Restore folds bare rows into the single listed account. Otherwise Settings lists them with Remove (`disconnect_source` allows it in that state). The downgrade steps now start with disconnecting each account. |
| 10 | Major | The design didn't consider one vault item per account. | FIX (adopted) | §2 adopts it. Alternative G records the per-field option as rejected. |
| 11 | Major | No risk register, and the MCP exposure wasn't stated. | FIX | Added a Risks and mitigations table, and Q-3 for work mail through Heron's MCP server. |
| 12 | Nit | The grep guard on outside sources was weaker than claimed. | FIX | `OUTSIDE_SOURCES` becomes private. U1's check also rejects `pub const OUTSIDE_SOURCES`. |
| 13 | Nit | Sign in again wasn't tied to the account it was started for. | FIX | The requested ID is stored in the sign-in record. A different `sub` fails with a fixed message. |
| 14 | Nit | The claim about two processes running the migration at once was wrong. | FIX | Only the process that owns background work runs the migration, called after `own_background_work()` in `serve` and `Cmd::Daemon`. |
| 15 | Nit | A missing client ID led to a sign-in path that wasn't defined. | FIX | `client_id: null` makes the account signed out, and Sign in again uses the Add client-ID order. |
| 16 | Nit | The sync cost figure was wrong. | FIX | 28 requests, not 27. |

## Structural

| # | Sev | Finding | Decision | Change |
|---|---|---|---|---|
| 1 | Major | G-3 overstated "no sign-in". | FIX | G-3 is scoped to accounts with a recorded `sub` and a vault token, and the three exceptions are named. |
| 2 | Major | "Upgrade pending" was defined for only one failure. | FIX | It is now defined by the old token still being present, whichever step failed. |
| 3 | Major | §12 stated an UNVERIFIED fact as true. | FIX | Reworded as unverified (fact M5). |
| 4 | Major | No Risks section. | FIX | Added, shared with design finding 11. |
| 5 | Nit | CUJ 5's examples were mixed between providers. | FIX | It now gives the message for each provider and points to §5a. |
| 6 | Nit | G-5's names didn't match the wire. | FIX | G-5 uses the wire names. The status is renamed `renewed` throughout (see style M2). |
| 7 | Nit | CUJ 1 said "connected" where it meant "listed". | FIX | Changed to "listed". |
| 8 | Nit | The diagram didn't match the text. | FIX | It now uses the `/api` paths and `ProviderSection`, and the Sync button is gone. |
| 9 | Nit | "Drops" meant two things. | FIX | Cancel "removes", and disconnect "marks failed". |
| 10 | Nit | Steps were referenced before the step table. | FIX | The table now comes first and the edge cases follow it. |
| 11 | Nit | ID prefixes were easy to confuse. | FIX | "fact Gn", "rule Q7" and "trial G9" are used consistently. |
| 12 | Nit | Cross-references lacked links. | FIX | Links added. Test seams moved into the plan. |
| 13 | Nit | The Q7 link pointed at the wrong doc. | FIX | It now points to `heron-v1.md#open-questions`, with the features §2 for the path list. |
| 14 | Nit | Q-2 lacked a fact G3 citation. | FIX | Added. |
| 15 | Nit | The console host was inconsistent. | FIX | Both hosts are noted. |
| 16 | Nit | CUJs didn't name their goals, and the cap had no CUJ. | FIX | Each CUJ now has a Goals line, CUJ 7 covers the cap, and the G-4 coverage note is added. |
| 17 | Nit | CUJs mixed When and Then. | FIX | CUJ 1 and CUJ 6 are restructured. |
| 18 | Nit | Test seams sat under NFR, and some named tests weren't gated. | FIX | Test seams moved into the Detailed Implementation Plan. Every named test is now in U1's check. |
| 19 | Nit | A `json` block had comments. | FIX | Retagged as `jsonc`. |

## Style

| # | Sev | Finding | Decision | Change |
|---|---|---|---|---|
| M1 | Major | "Listed" and "connected" were mixed up. | FIX | Both are defined in Terms and used consistently. |
| M2 | Major | One outcome had three names. | FIX | Renamed `refreshed` to `renewed` everywhere, including `Admission::Renewed`. |
| M3 | Major | The renewal notice contradicted Sign in again. | FIX | There are now two notices, one after Add and one after Sign in again. |
| M4 | Major | The status and the button had the same text. | FIX | The status reads "Signed out" and the button reads **Sign in again**. |
| M5 | Major | The hint was worded two ways. | FIX | One hint text, with ‹Provider›. |
| M6 | Major | The Admin console path differed between sections. | FIX | Unified to Security › Access and data control › API controls. |
| M7 | Major | UI copy said "this build" and "client". | FIX | Owner-facing strings reworded. |
| M8 | Major | Terms were undefined. | FIX | Added client, `sub`, tenant, vault, registry, item and work units to Terms. |
| M9 | Major | "Secret keys" was misleading. | FIX | Now "secret-store entries" and "record". |
| M10 | Major | The epoch definition was unclear. | FIX | Rewritten for the new rule: a new epoch on every sign-in, deleted on Disconnect. |
| M11 | Major | "Half-finished install" was contradictory. | FIX | Resolved by the new write order (registry first). |
| M12 | Major | Disconnect "drops" or "marks failed". | FIX | Marks failed. |
| M13 | Major | An unknown attempt returned 404 or expired. | FIX | It returns `expired` and is never 404. |
| N1-N13, N15-N17 | Nit | Wording, jargon and copy fixes. | FIX | Applied as suggested. N17 now names the `google_mail` and `google_calendar` functions, and trial G9 is defined. |
| N14 | Nit | Mixed British and American spelling. | DISMISS | UI copy keeps "organisation", matching the existing Heron copy. Quoted Microsoft labels stay verbatim. It has no effect on behaviour. |
