# Heron: several accounts per provider

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** Approved by the owner on 2026-11-23 ("approve #83 with defaults"). The code still needs the owner's review under rule Q7 before it merges.
*   **Date:** 2026-11-23
*   **Base:** PR #72 (`fix/mac-connect-sources`) at commit `614d681`. Every file and line cited here is from that commit, unless it says `main`.
    *   PR #72 has moved on since then, to `4e57087` on 2026-11-23. It adds a Mac Mail source, `mail_local`, to `SOURCES` and `OUTSIDE_SOURCES` and to the Email card's members.
    *   `mail_local` is a non-OAuth source, like the legacy `email`. It needs no per-account handling, and the family queries and the Email card include it as they include `email`.
    *   The line numbers cited here will move when the build rebases onto PR #72's final commit.

--------------------------------------------------------------------------------

## Overview

Today Heron can connect one Google account and one Microsoft account. Each provider has one set of entries in the secret store: `oauth_google_refresh_token` and its siblings. The core refuses a second account with "You signed in with a different account from the one connected" (`src/oauth.rs:234`).

This design lets each provider hold up to five accounts at once, such as a personal and a work Gmail. Each account gets:

*   its own secret-store record;
*   its own knowledge sources;
*   its own sync lock and its own epoch guard;
*   its own Disconnect, which removes only that account's items and memories.

The single account connected today moves to the new layout when Heron starts, with no new sign-in. A sign-in also reports why it failed, so a work account blocked by its admin shows a specific message instead of timing out.

Microsoft has no registered client yet. The Microsoft code paths change in the same way, and [§12](#12-microsoft-when-a-client-exists) is the plan for when a client exists, including one registered in a work tenant.

### Terms

*   **Client.** The app registration at the provider that Heron signs in through. For Google this is "Heron Mac", which has a client ID and is in Testing mode.
*   **`sub`.** The account identifier the provider puts in the ID token it issues to Heron's client. Heron uses it to tell accounts apart.
*   **Tenant.** A work or school organisation's Microsoft directory (Entra). An app registration lives in a tenant.
*   **Vault.** The operating system's credential store that `store_vault_only_secret` writes to: the Keychain on macOS, Credential Manager on Windows. The *fallback file* is the plaintext `secrets.json` that older builds could also write to.
*   **Registry.** The vault entry `oauth_<p>_accounts`: the list of account IDs for provider `<p>`.
*   **Listed, connected, signed out.**
    *   An account is *listed* when its ID is in the registry.
    *   It is *connected* when it is listed and its record holds a refresh token and a client ID, and the provider hasn't rejected the token (`AccountRecord::connected()`).
    *   A listed account that isn't connected is *signed out*. Its items and memories stay, and it has **Sign in again** and **Disconnect** buttons.
*   **Item.** One row of the `knowledge` table: one email or one calendar event.
*   **Epoch.** A random string kept in each account's record. Every successful sign-in gives the account a new epoch, and Disconnect deletes it. A sync or learning pass records the epoch when it starts, and commits only if the epoch is unchanged. That stops a slow network reply from writing data from before a disconnect or a new sign-in. This check is the *epoch guard*.
*   **`SOURCE_WRITES`.** A process-wide lock (`src/oauth.rs:17`). It is held while credentials are installed or rotated, while sync results are committed, and during disconnect.
*   **`OAUTH_LEARNING_COMMIT`.** An async lock (`src/oauth.rs:21`). It is held while a memory learned from an OAuth source is saved, and during disconnect.
*   **Transition lock.** `ServerState::oauth_transition` (`src/server.rs:46`). It orders sign-in start, callback and disconnect in the server.
*   **Taint, outside content.** Text written by other people: mail, calendar invitations, web pages.
    *   Once a conversation has read it, the conversation is *tainted* and actions need approval.
    *   Memories learned from it start *unconfirmed* (`OUTSIDE_SOURCES`, `src/memory.rs:18`).
*   **Rule Q7.** The owner's rule that security-sensitive pull requests wait for the owner's review ([heron-v1.md, Open Questions](heron-v1.md#open-questions)).
*   **Trial G9.** The owner's 14-day trial of using Heron day to day. No feature PR merges into `main` until the trial's PR has merged.
*   **Slint app.** Heron's cross-platform desktop app (`src/bin/desktop/`), separate from the SwiftUI Mac app.
*   **Work units, skein, testteeth, genesis.**
    *   *Work units* are the build tasks in the [Work units](#work-units) table.
    *   *skein* runs each unit as a separate build agent with its own check. Its `claude_build_swift` backend may run `swift build` and `swift test` outside the agent sandbox.
    *   *testteeth* grades tests by mutation testing.
    *   *genesis* audits evaluators.

## The Problem

*   **One account per provider.** The owner has a personal and a work Gmail, and may later have a personal and a work Microsoft account. Heron can read only one of each.
*   **Everything is keyed by provider:**
    *   the secrets (`Provider::secret_key`, `src/oauth.rs:66`);
    *   the knowledge sources (`google_email`, `google_calendar`, `microsoft_email`, `microsoft_calendar`; `src/oauth.rs:69-74`);
    *   the sync locks (`GOOGLE_SYNC`, `MICROSOFT_SYNC`; `src/oauth.rs:27-28`);
    *   the epoch (`Provider::account_epoch`);
    *   the HTTP routes (`/api/oauth/{provider}/…`; `src/server.rs:190-193`).

    A second account would overwrite the first account's credentials and mix its mail into the same source.
*   **Sign-in success is guessed.**
    *   The Mac app polls `GET /api/oauth` until the provider shows `connected` (`SettingsAndFirstRunStores.swift:216-218`). With one Google account already connected, the provider shows `connected` before a second sign-in has finished, so the app can't tell when that sign-in succeeded.
    *   The callback also throws away the reason a sign-in failed (`src/server.rs:1441`, `:1452`), so a blocked work account looks like a timeout.
*   **Google limits this build.** The "Heron Mac" client is in Testing mode, with billing off, and must stay free.
    *   Only addresses on its test-user list can sign in. The list holds 100 addresses at most.
    *   A test user's authorization expires seven days after consent ([facts G1, G2](research/heron-multi-account-provider-facts.md)).
    *   So every extra Gmail needs a console step, and every Gmail needs a new sign-in each week.

## Goals

*   **G-1, several accounts.** Up to five accounts per provider at once. The owner's personal and work Gmail sync side by side and appear as separate rows.
*   **G-2, separation.** A sign-in, sync, learning pass or disconnect for one account never reads or writes another account's record, items or memories. This holds even when they race.
    *   Learning may still link related facts across accounts, as it already links facts across sources today, but it never changes or retires another account's memories ([§3](#3-per-account-sources-with-taint-unchanged)).
    *   [Security invariants](#security-invariants-rule-q7) I-1 to I-3, I-8 and I-9 state this as tests.
*   **G-3, no forced sign-in.** An account connected today, with a recorded `sub` and its token in the vault (the owner's machine), keeps working after the upgrade. Its items and memories stay, and no new sign-in is needed. Three rarer old states end up signed out instead ([§8](#8-migrating-todays-single-account)):
    *   a token only in the fallback file;
    *   no recorded `sub`;
    *   no recorded client ID.
*   **G-4, taint unchanged.** Every per-account source is treated as outside content, exactly as its provider source is today (I-4).
*   **G-5, reported failures.** A sign-in ends as `added`, `renewed` (the account was already listed and Heron replaced its sign-in), `failed` with a fixed message, or `expired`. Admin blocks and "not a test user" get their own message, or a hint when the provider never redirects back.
*   **G-6, one-click weekly sign-in.** An account whose sign-in the provider stopped accepting shows as signed out. One click, with the account pre-selected, renews it and keeps its items and memories.

## Non-Goals

*   **A Microsoft client registration, or paying for one.** [§12](#12-microsoft-when-a-client-exists) is a plan only. Personal Microsoft mail comes through "Use your Mac's Mail" (PR #72), which this design doesn't touch.
*   **Several accounts in the Slint app** (`src/bin/desktop/setup_glue.rs`). Its provider-level calls keep working with one account per provider ([§9](#9-http-api)).
*   **Google verification or publishing to production.** Whether to switch the client to "In production" is the owner's call (Q-2). It is a console setting, not code.
*   **Merging the same mailbox read from two sources.** If one mailbox is read both through OAuth and through Mac Mail or IMAP, its items appear twice, as they do today.
*   **More than five accounts per provider, shared or team accounts, or choosing which calendars of an account to read.**
*   **A Linux vault.** OAuth stays macOS and Windows only (`src/server.rs:1379`).

## Open Questions

The owner resolved all four on 2026-11-23 by approving the defaults. Each entry keeps its alternative for the record.

*   **Q-1, account cap.** **Resolved: up to five accounts per provider.**
    *   Five accounts per provider. The cap is checked only when a new account is added, by `sub` ([§5](#5-sign-in-and-its-reported-result)).
    *   It bounds background sync, and the number of vault entries and rows in Settings. One Google account costs 28 HTTP calls per 30-minute cycle: one token refresh, one message list, 25 message reads and one calendar read (`google_mail`, `google_calendar`).
    *   The alternative is no cap, which leaves Google's 100-address test-user list as the only limit.
*   **Q-2, Google Testing or unverified production.** **Resolved: stay in Testing.** Both options are free.
    *   **Stay in Testing (chosen).** Each extra Gmail must be added as a test user, and each account is signed out every week. G-6 makes signing in again one click.
    *   **Switch to "In production" without verification** ([fact G3](research/heron-multi-account-provider-facts.md)):
        *   users click through Google's "unverified app" warning;
        *   the app may admit 100 new users over its whole lifetime;
        *   the test-user list is no longer needed;
        *   the weekly expiry probably ends, but no official page says so ([fact G3a](research/heron-multi-account-provider-facts.md), UNVERIFIED).

        Work accounts may be more likely to block an unverified app.
    *   The code is the same either way.
*   **Q-3, work mail through Heron's MCP server.** Heron's MCP server gives `search_knowledge` and `recall` to other apps the owner connects (`src/mcp.rs:232`). Adding a work Gmail makes that mailbox searchable by those apps too, and employer policy may forbid that. **Resolved: MCP-connected apps may search every connected account, work mail included, and the PR states the risk.**
    *   Today's behaviour is kept: every connected account is searchable.
    *   The alternative is a per-account "keep out of other apps" switch, under Future Improvements.
*   **Q-4, a server-side Cancel.** Today **Cancel** only stops the Mac app from waiting. If the owner then approves in the browser anyway, the account is still added ([§5](#5-sign-in-and-its-reported-result)). **Resolved: Cancel stays local, inside Heron.**
    *   Cancel stays local. The owner did approve the account at the provider.
    *   The alternative is a cancel route, which removes the waiting record so a later approval adds nothing. It needs an internal exchanging state that clients can see, and three more tests.

## Critical User Journeys (CUJs)

### CUJ 1: Add a work Gmail beside the personal one

*   **Goals**: G-1, G-2.
*   **Given**:
    *   the personal Gmail is connected and synced;
    *   the work address is on the client's test-user list ([§11](#11-google-test-users-and-workspace-blocks));
    *   fewer than five Google accounts are listed.
*   **When**: the owner opens Settings › Connections, clicks **Add another Google account**, picks the work account in Google's account chooser and allows access.
*   **Then**:
    *   Google showed its account chooser, because Heron sent `prompt=select_account consent`.
    *   The sign-in reports `added`, and a second Google row appears with the work address.
    *   Heron reads that account's mail and calendar into `google_email:<id>` and `google_calendar:<id>`.
    *   The personal account's items, memories and record are unchanged.

### CUJ 2: Add an account that is already listed

*   **Goals**: G-5.
*   **Given**: the personal Gmail is listed.
*   **When**: the owner clicks **Add another Google account** and picks the personal account again.
*   **Then**:
    *   The sign-in reports `renewed`, and no new row appears.
    *   The account's refresh token and epoch are replaced. Its items and memories stay.
    *   Settings shows "you@gmail.com is already in Heron. Heron renewed its sign-in."

### CUJ 3: Disconnect one account

*   **Goals**: G-2.
*   **Given**: the personal and work Gmail accounts are both connected.
*   **When**: the owner clicks **Disconnect** on the work row and confirms. The confirm sheet names the work address.
*   **Then**:
    *   Heron deletes the work account's record, items and memories, and removes it from the registry.
    *   It reports how many items and memories it removed.
    *   The personal account still syncs, and its data is unchanged.

### CUJ 4: Upgrade with one account connected

*   **Goals**: G-3.
*   **Given**: a build from before this change has one Google account connected, with a recorded `sub` and its token in the Keychain. This is the owner's machine today.
*   **When**: the owner installs the new build and opens Heron.
*   **Then**, before any sync:
    *   Heron makes one backup of `heron.db`;
    *   it writes the account's record and adds the account to the registry;
    *   it renames the `google_email` and `google_calendar` items and memories to `google_email:<id>` and `google_calendar:<id>`;
    *   it deletes the old entries.

    Settings then shows the same account, with nothing lost and no new sign-in.

### CUJ 5: A work account is blocked

*   **Goals**: G-5.
*   **Given**: the owner's organisation doesn't allow unapproved apps.
*   **When**: the owner tries to add the work account.
*   **Then**:
    *   **If the provider sends an error back to Heron,** Settings shows that provider's fixed message from [§5a](#5a-failure-messages). For Google that is the `admin_policy_enforced` message; for Microsoft it is the admin-approval message with its `AADSTS` code number.
    *   **If the provider stops on its own page and never redirects,** Settings shows the blocked-access hint from [§5a](#5a-failure-messages) from the start of the wait, and again with the timeout message.
    *   No account is added.

### CUJ 6: The weekly Google expiry

*   **Goals**: G-6.
*   **Given**:
    *   a Gmail account in Testing mode was authorized more than seven days ago;
    *   the background sync got `invalid_grant` when it tried to refresh the token, so the account is signed out.
*   **When**: the owner opens Settings, clicks **Sign in again** on that row, and finishes the sign-in in the browser with the address pre-filled.
*   **Then**:
    *   Before the click, the row showed "Signed out", and background sync skipped it.
    *   The sign-in reports `renewed`, and the row shows "Connected".
    *   Settings shows "Signed in again as you@gmail.com. Nothing Heron read from it was lost."

### CUJ 7: The cap

*   **Goals**: G-1 (Q-1).
*   **Given**: five Google accounts are listed.
*   **When**: the owner looks at the Google section.
*   **Then**:
    *   **Add another Google account** is disabled, with "Heron can connect up to five Google accounts. Disconnect one to add another."
    *   If a sixth account arrives anyway (for example from the Slint app), admission refuses it with the same text and writes nothing.

G-4 has no journey of its own: it is an invariant, covered by test I-4.

## Technical Plan (Architecture)

```text
 Mac app (SwiftUI)                       core (Rust, loopback HTTP)                         storage
 ─────────────────                       ──────────────────────────                         ───────
 ProviderSection ── Add / Sign in again ─▶ POST /api/oauth/{p}/start ─▶ oauth::begin ─▶ browser
        ▲                                   │ returns {authorization_url, attempt}
        │ poll                              ▼
        └── GET /api/oauth/sign-ins/{attempt} ◀── sign-in record ◀── GET /oauth/callback/{p}
                                                                              │ finish → install
                                                                              ▼
                                         registry   oauth_google_accounts = "68c0…,b71c…"   vault
                                         record     oauth_google_68c0… = {token, epoch, …}
 account row ── Disconnect ─▶ DELETE /api/oauth/{p}/accounts/{id}
                                          │ per-account lock, epoch guard
                                          ▼
                                         knowledge/memories source = google_email:68c0…   heron.db
```

### 1. Account identity

*   **Identity.** An account is the pair (provider, `sub`), as today. `sub` comes only from an ID token issued to this client (`account_subject`, `src/oauth.rs:191`).
    *   Google's `sub` belongs to the Google account.
    *   Microsoft's `sub` is pairwise: it depends on the client ID and the tenant ([fact M2](research/heron-multi-account-provider-facts.md)). [§12](#12-microsoft-when-a-client-exists) says what that means for a later change of client.
*   **The account ID.**
    *   It is the first 32 lowercase hex characters of SHA-256 over the UTF-8 bytes of `heron-oauth-account\n<provider>\n<sub>`, with no trailing newline. `<provider>` is `Provider::name()`: `google` or `microsoft`.
    *   Test vectors: (`google`, `user-1`) gives `68c0b45ff8d4a21e5bb2430d4507a770`, and (`google`, empty `sub`) gives `543e8f35a92d701c7f07c1fa4c05f4bf`.
    *   The ID is the same every time for the same account. So a second sign-in of an account finds the same ID (CUJ 2), and the upgrade migration can run again safely ([§8](#8-migrating-todays-single-account)).
    *   It never contains the raw `sub`.
    *   128 bits make a collision negligible, so there is no collision branch. The ID alone identifies the account.
*   **Label.**
    *   The ID token's `email`, or Microsoft's `preferred_username`, as `account_label` already reads it (`src/oauth.rs:211`). It may be absent; Settings then shows "Google account" or "Microsoft account".
    *   It is used for display, and as the `login_hint` when signing in again.
    *   It is never used to decide who signed in or whether a sign-in is allowed.
*   **`AccountRef`.** A new type in `src/oauth.rs`: the pair (provider, account ID). Every account uses one naming rule for its record and its sources. Today's single connection becomes an ordinary account through the migration ([§8](#8-migrating-todays-single-account)). No second runtime path for unmigrated connections is kept ([Alternative E](#alternative-e-keep-a-legacy-runtime-path-for-unmigrated-connections)).

### 2. Secret layout: one vault record per account

Each account is a single vault entry. Installing or renewing an account is then one verified write, and so is deleting it.

| Entry | Holds | Written with |
|---|---|---|
| `oauth_<p>_accounts` | the registry: account IDs separated by `,`, in the order they were added | `store_vault_only_secret` |
| `oauth_<p>_<id>` | the record, as JSON: `{"refresh_token": string or null, "client_id": string or null, "epoch": string, "label": string or null, "needs_sign_in": bool}` | `store_vault_only_secret` |

*   **Verified reads.** Every read of the registry or of a record uses a new `Config::vault_only_secret_checked(name) -> Result<Option<String>>`, which wraps `vault::get_checked` (`src/config.rs:897`).
    *   A read error aborts the operation. It is never treated as "absent".
    *   **No vault on this platform.** On a platform with no vault (Linux today), with a home that isn't enrolled in the test vault, the function returns `Ok(None)`: no vault-only entry can exist there. Today `vault::get_checked` always errors on such a platform (`src/config.rs:936-938`), so this rule is what keeps Email and Calendar learning from IMAP and ICS working on Linux.
    *   **Fixed error text.** The returned error carries a fixed message: "Heron couldn't read the Keychain. Try again." (on Windows, "the Credential Manager"). The OS's own text goes to the log only, because keyring errors can contain account names (`src/credential_transaction.rs:1-2`) and `ApiError` copies an error's full chain into the response (`src/server.rs:81-85`).
    *   **What each caller does with a read error:**
        *   `GET /api/oauth` returns that message with HTTP 503;
        *   `learn` skips the OAuth sources, logs the error, and still learns from the other sources;
        *   `sync_due` skips OAuth for that cycle;
        *   every other caller returns the error.
    *   Today's `Config::secret` returns `None` on any keyring error (`src/config.rs:683-685`, `:890-892`). Through it, a passing Keychain error could read as an empty registry, and a Disconnect would then write that empty list back and drop every other account.
*   **Verified writes.** Every write goes through `store_vault_only_secret`. That function writes, reads the value back, and restores the previous value on failure (`credential_transaction::replace`), and it never falls back to the plaintext file.
*   **Read-modify-write.** Registry changes, and record changes such as saving a rotated refresh token or setting `needs_sign_in`, happen under `SOURCE_WRITES` as read-check-write.
*   **Format checks.**
    *   Registry: each entry is trimmed and empty entries are skipped. An entry that isn't 32 lowercase hex characters is skipped and logged.
    *   Record: one that doesn't parse as JSON is treated as "no record" by **every** caller. The account shows signed out. Sign in again uses the Add client-ID order with no hint, and a renewal writes a fresh record. The Add client-ID order and `client_configured` skip that account, and migration step 5 leaves the record alone. So one damaged record never fails the whole provider.
*   **Listed without a record.** An account can be in the registry with no record. This happens when the registry entry was written first ([§5](#5-sign-in-and-its-reported-result), [§8](#8-migrating-todays-single-account)) and the record write then failed, or when a Disconnect stopped partway ([§6](#6-disconnect-one-account)). The account shows signed out, with Disconnect, and nothing can sync from it.
*   **Key form.** The account ID is 32 hex characters, so `oauth_<p>_<id>` can't clash with today's `oauth_<p>_<field>` entries. Only the migration reads those old entries.

### 3. Per-account sources, with taint unchanged

*   **Names.** An account's sources are `<base>:<id>`, for example `google_email:68c0b45ff8d4a21e5bb2430d4507a770`.
    *   This needs no schema change. `UNIQUE(source, ref)` (`src/store.rs:245`) keeps two accounts' message IDs apart.
    *   Per-account erase uses the existing `clear_knowledge(source)` and `delete_memories_by_source(source)` (`src/store.rs:1558`, `:1564`).
*   **Reading the suffix.** Three places read it. Nothing else splits a source name on `:`.
    *   **Helpers in `src/sources.rs`.**
        *   One returns the base of a source name.
        *   One maps a per-account source name to its `AccountRef`. It accepts only the four OAuth bases, followed by `:` and exactly 32 lowercase hex characters.
    *   **`learn_one`** (`src/sources.rs:488`).
        *   It accepts a name in `SOURCES` other than the four bare OAuth bases, or a valid per-account name.
        *   A bare OAuth base is refused with "Unknown source", because it belongs to no account and so has no epoch to guard it.
        *   `learn` leaves bare OAuth bases out of the Email and Calendar cards.
    *   **The store's family match** (Aggregates, below).
*   **`disconnect_source`.** It keeps its exact `SOURCES.contains` check (`src/server.rs:1578`), so a per-account name gets "Unknown source" (HTTP 404) there.
    *   Bare OAuth names are refused only while that provider's old `oauth_<p>_refresh_token` still exists, because the migration still needs those rows (`src/server.rs:1585-1595`).
    *   Once the migration has finished, no listed account uses bare names. So the route can erase bare rows brought back by restoring a pre-upgrade backup ([§8](#8-migrating-todays-single-account), "Restore").
    *   The refusal at `:1593`, for the legacy `email` and `calendar` sources while an OAuth account is connected, now checks "any listed account of either provider". The legacy sources belong to no provider, and a signed-out account still counts.
*   **Taint.** Taint is set per tool, not per source: reading `search_knowledge` or `upcoming_events` taints the conversation whatever the source (`src/tools/mod.rs:159`, `src/agent.rs:695`). So reads are unchanged. Two things look at memory sources:
    *   **Outside checks.**
        *   `memory::is_outside(source)` matches the base of the name against `OUTSIDE_SOURCES`.
        *   It replaces the two `OUTSIDE_SOURCES.contains` checks: at `src/memory.rs:278` (finding an unconfirmed outside row near a new save) and at `:369` (a user restating an outside memory confirms it).
        *   Without it, `:278` would let outside text through: a model save that echoes `google_email:<id>` text could become a confirmed memory.
        *   `OUTSIDE_SOURCES` becomes private to `memory.rs`, so code elsewhere can only call `is_outside`.
    *   **Trusted and vouching checks stay exact.** `TRUSTED_SOURCES.contains` and `VOUCHING_SOURCES.contains` (`src/memory.rs:263-264`, `:313`) keep matching exactly. A name with a suffix is never trusted and starts unconfirmed, which is the safe direction.

    Test I-4 covers both. The U1 check requires exactly one `OUTSIDE_SOURCES.contains` in `src/`: the one inside `is_outside`.
*   **One account's memories never change another's.**
    *   **The problem.** `save_detailed` looks for a near match across all sources. A match can update the other row, or retire it (`src/memory.rs:270-301`, `:330-338`, `:363-383`). With two accounts of one provider, a calendar invitation sent to both the personal and the work calendar would make one account's learning pass change, or retire, the other account's memory. Neither account's epoch guards that.
    *   **The fix.** A candidate row is left out when `sources::account_of_source` gives an account for both the save's source and the row's source, and the two accounts differ. Rows from the same account stay candidates, even across its mail and calendar. Bare-named rows stay candidates as today, because they belong to no account. The rule applies on all three candidate paths:
        *   the vector scan (`src/memory.rs:270-277`);
        *   the outside-row scan (`:288-300`);
        *   the `replaces` target (`:333-337`).

        Both accounts then keep their own copy, and each copy goes when its own account is disconnected.
    *   **What stays the same.** Matching against every other source, including the user's own facts, works as today. Test I-9 covers this.
*   **Aggregates.** New store functions match a source *family*: the bare base, or the base followed by `:`. The existing exact-match functions stay for every other caller. The match isn't a `LIKE` pattern, because `_` in `google_email` is a `LIKE` wildcard.
    *   `connected_summary` (`src/sources.rs:628-629`) and `get_sources` (`src/server.rs:1486-1487`) count by family.
    *   `search` (`src/sources.rs:608-616`) uses the family search for the `email` and `calendar` aliases, and for a single OAuth base such as `google_email`. Other single sources keep the exact search.
    *   `upcoming` (`src/sources.rs:252-257`) runs one family query per calendar base, then sorts and truncates to `limit` as today.

    None of them needs the registry or a vault read. That matters, because `connected_summary` feeds the system prompt and `search` and `upcoming` back agent tools.

    Disconnect deletes an account's items, so a family match finds only listed accounts' data, plus any bare rows still waiting for the migration. `learn` (`src/sources.rs:460-462`) is the one aggregate that reads the registry: it learns only from connected accounts, because it must check each account's epoch.
*   **Labels.** `source_label` (`src/sources.rs:444`) and the upcoming-events label (`:258-262`) match on the base. Search results show the raw source name, as today. Showing the account's address instead is under [Future Improvements](#future-improvements).
*   **Memory tags.** `learn_one` passes the source as the memory's tags (`src/sources.rs:557`), so new memories carry the per-account name. The migration rewrites tags that equal the bare base, alongside the source ([§8](#8-migrating-todays-single-account), step 5).

### 4. Sync locks and epoch guards per account

*   **Locks.** `GOOGLE_SYNC` and `MICROSOFT_SYNC` become one lock per account, created on first use and keyed by provider and account ID.
    *   Entries are never removed, which is harmless at five accounts per provider.
    *   Each account holds its lock for the whole `sync`, from the token refresh to the final commit, as today.
    *   Two accounts of one provider sync at the same time, each refreshing only its own token. That matters for Microsoft, which returns a new refresh token on every use ([fact M6](research/heron-multi-account-provider-facts.md)).
*   **Epochs.**
    *   `access_token_with_generation` reads the account's record with a verified read.
    *   Under `SOURCE_WRITES`, it checks that the record's refresh token and epoch are unchanged before saving a rotated token (today `src/oauth.rs:310-321`, now per account).
*   **Commits.** Under `SOURCE_WRITES`, `sync` checks two things:
    *   the account's epoch still equals the one captured with its access token;
    *   the account is still connected.

    Only then does it call `replace_knowledge_snapshot` on the account's two sources.
    *   A disconnect deletes the record, and every sign-in writes a new epoch. So a commit after either one is dropped.
    *   Another account's changes don't touch this account's record, so they never block it.
*   **Learning.**
    *   `learn_one` maps the source to its account and captures that account's epoch before calling the model.
    *   It re-checks the epoch under `OAUTH_LEARNING_COMMIT` before each memory save (today `src/sources.rs:493-556`, now per account).
    *   Because renewal also writes a new epoch, a pass that started before a disconnect and a new sign-in can't save afterwards.
    *   `OAUTH_LEARNING_COMMIT` stays global. A disconnect of one account waits for any provider memory save in progress, which is short and keeps the reasoning about lock order simple.
*   **Background sync.** `sync_due` (`src/sources.rs:577`) runs `oauth::sync` for each connected account in turn. Signed-out accounts aren't connected, so it skips them.

### 5. Sign-in and its reported result

*   **Start.** `POST /api/oauth/{p}/start` takes `{client_id?, account_id?}`. It doesn't check the cap; admission does, by `sub` (below).
    *   **With `account_id`** (Sign in again): the ID must name a listed account, or start returns 404. The server uses that account's stored client ID, or the Add order below when the record has none. It sends the account's label as `login_hint` when there is one, and keeps the ID in the sign-in record.
    *   **Without `account_id`** (Add), the client ID is the first of these. The order is today's (`src/server.rs:1391-1394`), with "stored" now meaning a listed account's record:
        1.  `client_id` from the body;
        2.  the stored client ID of the most recently added listed account that has one;
        3.  the bundled ID (`HERON_<P>_OAUTH_CLIENT_ID`);
        4.  otherwise, the existing "no public desktop OAuth client ID" error.
    *   **`client_configured`** in `GET /api/oauth` is true when a bundled ID exists or any listed account's record has a client ID. That matches today, where disconnect also clears the provider's stored client ID.
    *   **Prompt.** `oauth::begin` sends the `prompt` value below.

    | Request | Google `prompt` | Microsoft `prompt` | `login_hint` |
    |---|---|---|---|
    | Add an account | `select_account consent` | `select_account` | none |
    | Sign in again, label known | `consent` | none | the account's address |
    | Sign in again, no label | `select_account consent` | `select_account` | none |

    The account chooser stops the browser from quietly reusing whichever account it is already signed in to ([fact G6](research/heron-multi-account-provider-facts.md)).
*   **Sign-in records.** `oauth_pending` becomes one map of sign-in records, keyed by a random 128-bit `attempt` handle. Start returns the handle; it is never sent to the provider.
    *   **States.**
        1.  `waiting` holds the provider, the OAuth `state`, the `Pending` data, and for Sign in again the requested account ID.
        2.  While the callback redeems the code, the record is marked as exchanging. This is internal only: a poll still reports `waiting`. The callback finds the record by `state`, a scan of at most 16 records.
        3.  `added`, `renewed` or `failed` is the result. `added` and `renewed` also carry `first_sync`, which is `running`, `done` or `failed`, plus a message when it failed.
    *   **Replays.** A second callback with the same `state` finds no waiting record, and gets the existing "expired or was already used" page.
    *   **Retention.** A `waiting` record expires 10 minutes after start, as today. A finished record stays for 10 minutes after it finished, so a late poll still sees the result.
    *   **Cap.** The map holds at most 16 records, finished ones included. Today's limit is 8 waiting sign-ins (`src/server.rs:1410`); it rises to 16, because finished records now stay for 10 minutes. Start removes expired records first. If 16 remain, it returns HTTP 429 with the existing "Too many unfinished sign-ins. Try again later."
    *   **Locking.** Polling takes only the map's own mutex. It never waits for the transition lock, which the callback holds during the token exchange (`src/server.rs:1430-1444`).
    *   **Cancel is local.**
        *   In the Mac app, **Cancel** stops polling and clears the waiting state, as today. The server's record is left to expire after 10 minutes.
        *   If the owner still approves in the browser after clicking Cancel, the account is added, because the owner did approve it at the provider. It appears at the next refresh of Settings.
        *   A server-side cancel is owner call Q-4.
*   **Admission.** It is decided by `sub` alone, under `SOURCE_WRITES`. `admit` returns `Ok(Admission::Added)`, `Ok(Admission::Renewed)` or an error with a fixed message. `count` is the number of listed accounts of that provider.

    | Sign in again for a different ID? | `id(sub)` listed? | `count` | Result | What it does |
    |---|---|---|---|---|
    | yes | any | any | error | "You signed in as a different account. Choose ‹label› in the account chooser." Writes nothing. |
    | no | yes | any | `Renewed` | Writes the full record: the new token, client ID and label, a new epoch, and `needs_sign_in: false`. If the new ID token has no label, the old label is kept. Items and memories stay (CUJ 2, CUJ 6). |
    | no | no | `>= MAX_ACCOUNTS_PER_PROVIDER` | error | "Heron can connect up to five ‹Provider› accounts. Disconnect one to add another." |
    | no | no | `< MAX` | `Added` | Adds the ID to the registry, then writes the full record with a new epoch. |

    *   **Write order.** For `Added`, the registry entry goes first. A failed record write then leaves a signed-out row with a Disconnect button, never a live token that nothing lists.
    *   **The old one-account refusal goes away** (`src/oauth.rs:234`), because a different account is now a new account.
*   **Callback** (`/oauth/callback/{p}`).
    *   **On success.**
        *   The record becomes `added` or `renewed`, with the account's ID and label.
        *   The callback spawns the first sync for that account only, as today, and sets `first_sync` when it ends. The Mac app doesn't call sync after a sign-in.
        *   The page says "Account connected" for `added`, or "Account renewed" for `renewed`, then "You can close this tab and return to Heron."
    *   **On failure.** An `error` query parameter, or a failure inside `finish`, makes the sign-in record `failed` with a fixed message ([§5a](#5a-failure-messages)), and the page shows the same message.
        *   A callback with neither `code` nor `error` ends the same way, with the generic message.
        *   So does a callback whose provider doesn't match the record's, as today (`src/server.rs:1438-1440`).
    *   **Provider text.** Text the provider sent (`error_description`) is never rendered, logged or returned (I-6).
*   **Polling.** `GET /api/oauth/sign-ins/{attempt}` returns the record ([§9](#9-http-api)). An unknown or expired handle returns `{"status": "expired"}`; it isn't an error. The Mac app polls this instead of guessing from `connected`.

#### 5a. Failure messages

These are fixed strings in `src/oauth.rs`. They are display only: no logic depends on a provider's code, which Microsoft says can change ([fact M3](research/heron-multi-account-provider-facts.md)).

*   **Microsoft codes.** The `AADSTS` number is read from `error_description`. Only its digits are kept, and the rest of the text is discarded. The digits are needed to tell an admin block from a decline.
*   **Google errors.** Only the `error` value is used.

| Provider signal | Message |
|---|---|
| Google `error=access_denied` | "Google stopped the sign-in. You may have declined access, or this address isn't on Heron's test-user list yet." |
| Google `error=admin_policy_enforced` | "Your organisation's Google admin doesn't allow Heron. An admin can mark Heron as trusted in the Google Admin console, under Security › Access and data control › API controls." |
| Google `error=org_internal` | "Heron's Google sign-in is set up to accept accounts from one organisation only." |
| Microsoft 65001, 90093, 90094, 900941 | "Your organisation needs an admin to approve Heron before this account can sign in (Microsoft code AADSTS‹n›)." |
| Microsoft 65004, or `access_denied` with no code | "You didn't allow access." |
| Any other Microsoft code | "Microsoft stopped the sign-in (code AADSTS‹n›)." |
| A Heron-side failure from the closed list below | that entry's fixed text |
| Anything else, including network errors, unreadable replies and vault errors | "The sign-in couldn't be completed. Try again." |

*   **The closed list.** `finish` and `install` return a typed `SignInError`. Its variants carry today's fixed strings:
    *   token exchange rejected (`src/oauth.rs:169`, with the HTTP status number);
    *   Google consent incomplete, or scopes missing (`:245`, `:252`);
    *   no offline access (`:257`);
    *   issued to a different app (`:203`);
    *   the provider didn't say which account (`:192`);
    *   expired or invalid code (`:180`);
    *   the cap message;
    *   the "different account" message for Sign in again.

    Every other error, including `reqwest` failures and vault errors whose text comes from the OS, becomes the generic message. Only the variant's own text is shown, never an error's source chain. Vault errors are logged with their chain, as today, and a log line never includes provider text.
*   **Blocks that never redirect.** Google may stop on its own page and never redirect back ([fact G5](research/heron-multi-account-provider-facts.md)), and so may Microsoft's blocked-consent screen ([fact M4](research/heron-multi-account-provider-facts.md)). So the Mac app shows this hint while waiting, not only after the timeout:

    > If ‹Provider› says access is blocked, this address may not be on Heron's test-user list yet, or your organisation may not allow Heron.

    The hint has a "How to fix this" disclosure with the steps from [§11](#11-google-test-users-and-workspace-blocks).

### 6. Disconnect one account

`oauth::disconnect(app, &AccountRef) -> (items, memories)` runs under `SOURCE_WRITES`. Its first step makes the account unable to sync.

*   **Order:**
    1.  Delete the account's record from the vault, verified with `delete_verified`. This removes the token and the epoch together, so any sync or learning pass in progress for this account can no longer commit.
    2.  Delete the account's two sources from `knowledge`, and their memories.
    3.  Remove the ID from the registry.
    4.  If the ID is the migration's target ([§8](#8-migrating-todays-single-account)) and an old `oauth_<p>_refresh_token` still exists, also delete the old token, the old entries, the bare-named items and memories, and any items and memories under the target ID's own per-account sources. Count them in the result. That covers a migration that renamed the rows (step 3) and then stopped before the account was listed (step 4).
*   **On failure.**
    *   Every step's error is returned.
    *   If step 1 fails, nothing has changed.
    *   If a later step fails, the account stays listed and signed out, so Disconnect can be retried, and the retry finishes the remaining steps.
*   **The HTTP side** (`disconnect_oauth`) keeps two steps from today: it takes the transition lock, and it waits for `OAUTH_LEARNING_COMMIT`.

    It now marks every `waiting` sign-in record for that provider as `failed`, with "This sign-in was cancelled because a ‹Provider› account was disconnected. Start the sign-in again." (Today it drops them.)
*   **Why every waiting sign-in for the provider is cancelled.** A waiting sign-in can't be tied to an account until its callback arrives. So the only way to stop a delayed callback from re-adding the account just removed is to cancel them all. The cost is that an unrelated "add account" in progress has to be started again.
*   **What Disconnect doesn't reach.** Copies of `heron.db` in `~/.heron/backups/` keep whatever they held when they were made. That includes the one-time migration backup. The [risk table](#risks-and-mitigations) covers this.

### 7. Signed out after expiry or revocation

*   **When the flag is set.** `redeem` returns only the token endpoint's HTTP status and its JSON `error` value. A refresh can come back with HTTP 400 and `error` equal to `invalid_grant`, from either provider. Then, under `SOURCE_WRITES`, `access_token_with_generation` sets `needs_sign_in: true` in the record, but only if the record's refresh token is still the one that was rejected and its epoch is unchanged.

    That stops a late rejection of an old token from signing out an account that was just renewed.
*   **The message.** "‹Provider› signed Heron out of this account. Sign in again so Heron can keep reading it. Nothing Heron learned from it is lost."
*   **Causes:**
    *   Google's seven-day expiry in Testing mode ([fact G1](research/heron-multi-account-provider-facts.md));
    *   a user, Workspace admin or tenant admin revoking access;
    *   a change of Microsoft client ([§12](#12-microsoft-when-a-client-exists)).
*   **Effect.** The account stays listed, with its data, and shows signed out. Background sync skips it. A `renewed` sign-in clears the flag.

### 8. Migrating today's single account

*   **When it runs.** `oauth::migrate_legacy_accounts(&App)` is a plain function, not `async`. `App::own_background_work()` calls it at the moment it first gets the background lock, next to `recover_interrupted_approvals` and `recover_interrupted_jobs` (`src/app.rs:2112-2114`).
    *   That one call site covers every way Heron starts:
        *   the Mac app's `heron serve --print-endpoint`, through `start_endpoint` (`src/main.rs:823`, `src/server.rs:261`);
        *   `heron daemon` (`src/main.rs:846`);
        *   a Heron that takes over from one that stopped (`src/app.rs:2134`).
    *   Only the one Heron that owns background work migrates. A second Heron, such as a CLI command run while the app is open, doesn't.
    *   At first start the migration runs before the loops and the HTTP listener. On a takeover the listener may already be up. Then `SOURCE_WRITES`, which the migration holds, keeps it and any sign-in or sync from overlapping, and `upgrade_pending` covers the state in between.
    *   The lock order is fixed: `own_background_work`'s own mutex, then `SOURCE_WRITES`. Nothing takes them the other way round.
*   **Per provider.**
    *   Each provider is handled on its own.
    *   Each provider's whole run holds `SOURCE_WRITES`, and no step awaits.
    *   A failure stops that provider's run, is logged, and is retried the next time a Heron first gets the background lock.
    *   With no old entry, step 1 is a single vault read and nothing else happens.
*   **Why it isn't one transaction.** The vault and SQLite can't commit together. So each step can be repeated, and step 6 is the one point after which a provider counts as migrated.
*   **Why the rename comes first.** A record exists only after the rename has committed. So nothing can sync while bare rows are still waiting to be renamed, and the migration uses the same registry-before-record order as `Added`.

| Step | Action | Rule if it fails, or Heron stops here |
|---|---|---|
| 1 | Read the old entries with `legacy_entry(cfg, p)`. It reads each old field (`refresh_token`, `account_subject`, `client_id`, `account_label`) with a verified vault read, then from the fallback file through a new `Config::fallback_file_secret(name) -> Result<Option<String>>`. If there is no old refresh token in either place, this provider is done. | Any read error stops this provider's run. An unreadable old `sub` can therefore never file an account under the wrong ID. |
| 2 | Before the first provider that needs migrating, back up the database with the label `before-multi-account` (`snapshot`, `src/store.rs:311`), **unless a usable `before-multi-account` copy already exists** in the backups folder. "Usable" means a file matching `heron-before-multi-account-*.db` that opens read-only and passes the store's existing `damage` check (`src/store.rs:279`). A copy that doesn't pass is left where it is, and a new copy is made, so a backup cut short by a crash never counts. A usable copy is never pruned. So a run that stops after step 3 and runs again later can't replace the true pre-upgrade copy with a renamed one. An in-memory store (tests only) has nothing to back up, so the step is skipped there. | A failed backup stops both providers' runs. |
| 3 | Compute `id = account_id(p, old subject, or "" when none)`. In one SQLite transaction (`rename_sources`), for both bases:<br>• rename `<base>` to `<base>:<id>` in `knowledge.source` and `memories.source`;<br>• set `memories.tags` to `<base>:<id>` where it equals `<base>`.<br>A bare `knowledge` row whose (new source, ref) already exists is deleted, and the already-renamed row is kept. | All or nothing |
| 4 | Add `id` to the registry if it is missing. | Until step 5 succeeds, the account is listed without a record, so it shows signed out. |
| 5 | If no record for `id` exists (verified read), write one built from the old entries (below). If a record is already there from an earlier, half-finished run, leave it. | It never overwrites an existing record (I-5). |
| 6 | **Commit point:** delete the old refresh token from the vault (`delete_vault_only_secret`) and from the fallback file, as `disconnect` does today (`src/oauth.rs:342-347`). | If this fails, the next run repeats steps 1 to 6. Each step finds nothing left to do until step 6. |
| 7 | Clear the other old entries (best effort, as `disconnect` does today). | Logged |

*   **The record built in step 5:**
    *   `refresh_token`: the old token, but only when it is in the vault;
    *   `client_id`: the old value, or `null`;
    *   `label`: the old value, or `null`;
    *   `epoch`: new and random;
    *   `needs_sign_in`: `false`.
*   **A platform with no vault** (Linux today). Steps 3 to 5 are skipped, because the registry and the record can't be written there. The bare rows and the old entries stay where they are, and `upgrade_pending` stays true. `DELETE /api/oauth/{p}` removes them. OAuth sign-in has never worked on such a platform (`src/server.rs:1379-1382`), so only very old builds can have left anything behind.
*   **The rare old states.** Each ends signed out (see G-3).
    *   **A token only in the fallback file.** Today's code already refuses to use such a token (`src/oauth.rs:298-299`). The record gets no token, so the account is signed out. Its items and memories stay, and signing in again reports `renewed` if `sub` was recorded.
    *   **No recorded `sub`.** The account is filed under the fixed ID `account_id(p, "")`, and it keeps syncing while its token works. A later sign-in with that account gives a real `sub`, so the account appears as a second row, and the owner disconnects the old one. §12 accepts the same outcome for a change of Microsoft client.
    *   **No recorded client ID.** The record has `client_id: null`, and the account shows signed out. Sign in again then uses the Add client-ID order ([§5](#5-sign-in-and-its-reported-result)).
*   **Running it again.** Every step does nothing the second time:
    *   step 2 finds the copy already made;
    *   steps 3 and 4 find nothing to rename or add;
    *   step 5 never overwrites.
*   **Upgrade incomplete.** This is the state while the old `oauth_<p>_refresh_token` still exists, whichever step stopped.
    *   `GET /api/oauth` sets `upgrade_pending: true` for that provider and lists whatever the registry holds.
    *   Settings shows: "Heron couldn't finish upgrading your ‹Provider› account. It will try again the next time Heron starts."
    *   Adding other accounts works.
    *   "The migration's target" is `id` as step 3 computes it from `legacy_entry`, which `upgrade_pending` and disconnect also use.
    *   Disconnecting the migration's target ID, or calling `DELETE /api/oauth/{p}` when no account is listed, also removes the old token, the old entries, and the bare-named items and memories ([§6](#6-disconnect-one-account), step 4). So a disconnected account can't come back at the next run.
*   **Restore.** `heron restore` of a backup taken before the upgrade, run under the new build, brings back bare-named rows but no old token.
    *   They aren't renamed into any account, because they might belong to an account the owner has since disconnected.
    *   They stay readable through the family queries, and `DELETE /api/sources/<base>` erases them ([§3](#3-per-account-sources-with-taint-unchanged)).
    *   The README says the `before-multi-account` copy is meant only for the downgrade path below.
*   **What the owner does first.** The owner's `heron.db` is live and holds real mail. Step 2 makes the backup automatically. The owner can also run `heron backup` before installing the new build.
*   **Downgrade.**
    1.  In the new build, disconnect each account, which deletes its vault record.
    2.  Run `heron restore` with the `before-multi-account` backup.
    3.  Sign in again under the old build.

    The PR description says so. If the owner downgrades without disconnecting first, each per-account record stays in the vault, unused, until it is disconnected in a later new build.

### 9. HTTP API

| Route | Change |
|---|---|
| `GET /api/oauth` | Each provider entry has the shape below. The provider's `connected` is true when any account is connected. An account's `needs_sign_in` is true when it is signed out ([§7](#7-signed-out-after-expiry-or-revocation)). No token, `sub` or epoch is returned (I-7). The old single `account` field is removed. No Rust client reads it: the Slint app reads only `connected` (`src/bin/desktop/setup_glue.rs:991-998`). |
| `POST /api/oauth/{p}/start` | Takes `{client_id?, account_id?}` ([§5](#5-sign-in-and-its-reported-result)). Returns `{provider, authorization_url, attempt, expires_in_seconds: 600}`. |
| `GET /api/oauth/sign-ins/{attempt}` | New. Returns the sign-in record. |
| `DELETE /api/oauth/{p}/accounts/{id}` | New. Disconnects one account. Returns `{provider, account_id, removed_items, removed_memories}`. |
| `POST /api/oauth/{p}/sync` | Kept for the Slint app. Syncs every connected account of that provider in turn. If all succeed, it returns `{provider, emails, events}` with the totals. If any fails, it returns the first error after trying every account; the accounts that succeeded keep their new data. |
| `DELETE /api/oauth/{p}` | Kept for the Slint app.<br>• **No listed account:** removes any old pre-upgrade data ([§8](#8-migrating-todays-single-account)) and returns the counts, or zeros.<br>• **One listed account:** disconnects it.<br>• **Two or more:** refuses with HTTP 409, "Several ‹Provider› accounts are set up in Heron. Disconnect them one at a time in the Heron Mac app's Settings."<br>The Slint app already shows a failed disconnect's message (`setup_glue.rs:1105-1107`). |
| `GET /api/sources` | `oauth: {google, microsoft}` stays a boolean meaning "any account connected". Counts add up every account's sources by family ([§3](#3-per-account-sources-with-taint-unchanged)). |

**Shapes.** These literals are the contract between U1 and U2. U2's decoding tests decode exactly these literals, and U1's tests check the same key sets. Optional fields are always present, as `null` when they are empty.

```jsonc
// GET /api/oauth, one provider entry
{"provider": "google", "client_configured": true, "read_only": true, "connected": true,
 "max_accounts": 5, "upgrade_pending": false,
 "accounts": [{"id": "68c0b45ff8d4a21e5bb2430d4507a770", "label": "you@gmail.com", "connected": true, "needs_sign_in": false}]}

// GET /api/oauth/sign-ins/{attempt}
{"status": "added", "provider": "google", "account_id": "68c0b45ff8d4a21e5bb2430d4507a770", "label": "you@gmail.com",
 "message": null, "first_sync": "running", "first_sync_message": null}
// status: waiting | added | renewed | failed | expired.
// first_sync is null unless status is added or renewed.
```

*   **Errors** use the existing `ApiError` body, `{"error": "<message>"}` (`src/server.rs:75-85`), with these statuses:
    *   **400**, the existing default conversion: a bad request body, or a sync that failed, including the [§7](#7-signed-out-after-expiry-or-revocation) message.
    *   **400** also for an unknown provider, as today (`Provider::parse`, `src/server.rs:1383`).
    *   **404**: an account ID that is malformed or not listed. An unknown `attempt` handle isn't an error: polling returns `{"status": "expired"}`.
    *   **503**: a vault read error ([§2](#2-secret-layout-one-vault-record-per-account)), with the fixed Keychain message.
    *   A body that isn't valid JSON is refused by axum's `Json` extractor before any handler runs, as it is today, so it doesn't get the `{"error": …}` body.
    *   **409**: the provider-level delete with several accounts.
    *   **429**: the sign-in map is full.

    Sync errors keep today's text. Their chain never holds provider text, because `get_json` and `redeem` carry only the HTTP status and the `error` value.
*   **Account IDs in paths** must match `^[0-9a-f]{32}$` and name a listed account of that provider.
*   **Authentication.** All `/api/*` routes stay behind the existing token and Origin checks. The callback stays on its current loopback route, which takes no token (`src/server.rs:209-211`).
*   **The Slint app's connect** sends no `account_id`, so it uses the Add prompt and may add a second account. That account appears in the Mac app, and the Slint app's own Disconnect then gets the 409 message. This is accepted, because multi-account support in the Slint app is a non-goal.

### 10. Mac app

The base is PR #72's `ConnectionsViews.swift`. `AccountRow` becomes `ProviderSection`, one per provider.

*   **Rows.** Header "Google", then one row per account. Each row shows:
    *   the address, or "Google account" when there is no label;
    *   a status, "Connected" or "Signed out";
    *   its own spinner and error line;
    *   **Sign in again** (only when signed out), which starts a sign-in with that row's `account_id`;
    *   **Disconnect**.
*   **Notices.** When `upgrade_pending` is true, the section shows the [§8](#8-migrating-todays-single-account) upgrade notice.
*   **Under the rows.**
    *   **Sign in with Google** (primary) when there are no accounts.
    *   **Add another Google account** (secondary) when there are some.
    *   At the cap, the button is disabled with "Heron can connect up to five Google accounts. Disconnect one to add another."
    *   The "isn't set up in this build" note stays.
*   **Waiting.**
    *   "Waiting for you to finish signing in…", **Cancel**, and the blocked-access hint with its "How to fix this" disclosure ([§5a](#5a-failure-messages)).
    *   **Cancel** stops polling and clears the waiting state, as today ([§5](#5-sign-in-and-its-reported-result), "Cancel is local").
*   **Results.**
    *   **`added`**: the new row appears with "Reading your email and calendar…". The spinner runs until the record's `first_sync` is no longer `running`. The app keeps polling the record for that, and doesn't call sync itself.
    *   **A failed first sync** shows "Signed in, but the first read failed (‹message›). Heron will try again later."
    *   **`renewed` after Add**: "‹address› is already in Heron. Heron renewed its sign-in."
    *   **`renewed` after Sign in again**: "Signed in again as ‹address›. Nothing Heron read from it was lost." Both `renewed` cases show the same first-sync spinner.
    *   **`failed`**: the message, in that provider's error line.
    *   **`expired`, or the timeout**: the existing timeout text, plus the hint.
*   **Disconnect confirm.** "Heron will sign out of ‹address› and delete the email, calendar events and facts it learned from this account. Your other ‹Provider› accounts stay connected. To also remove Heron's access at ‹Provider›, remove Heron in this account's security settings."
*   **First run.** The first-run sources step already shows the same `ConnectionsPanel`, so it gets this layout too.
*   **Store state.** In `SettingsStore`:
    *   `oauthAccounts: [OAuthAccount]` becomes `oauthProviders: [OAuthProviderStatus]`;
    *   `signingIn` keeps one sign-in at a time across all providers, and holds the `attempt` handle;
    *   `syncingAccounts`, the busy map and the error map are keyed by `"<provider>:<id>"`;
    *   `pendingDisconnect` becomes `enum PendingDisconnect { case source(ConnectionInfo), case account(OAuthProviderID, OAuthAccount) }`. `confirmDisconnect` sends `.account` to `disconnectOAuth(provider:accountID:)` and `.source` to `disconnectSource`. `SourceCopy.forgetSentence` gets an account form that names the address. Today the code tells OAuth rows apart by `OAuthProviderID(rawValue: row.id)` (`SettingsAndFirstRunStores.swift:313-318`), which would send a per-account row to `disconnectSource`.
    *   If the sign-in record expires while `first_sync` is still `running`, the app stops the spinner and reloads Settings.

### 11. Google test users and Workspace blocks

*   **Adding a Gmail.** These owner steps also go in the PR description and in `packaging/macos/README-google-signin.md`.
    1.  Open the Google Auth Platform's Audience page for the Google Cloud project that owns the "Heron Mac" client (`console.cloud.google.com/auth/audience?project=<project-id>`; the owner's report names the project), signed in as the owner. Google's own page links the same path under `console.developers.google.com` ([fact G2](research/heron-multi-account-provider-facts.md)).
    2.  Under **Test users**, choose **Add users**, enter the address, and save. The list holds 100 addresses.
    3.  In Heron, choose **Add another Google account**.
*   **Weekly expiry.** In Testing mode, each account's authorization ends seven days after consent. CUJ 6 is the weekly path, and Q-2 is the alternative.
*   **Workspace accounts.**
    *   A Workspace admin may block third-party apps, or restrict high-risk scopes; `gmail.readonly` is one. Google then shows `admin_policy_enforced` ([fact G4](research/heron-multi-account-provider-facts.md)).
    *   The fix is the admin's. In the Admin console: Security › Access and data control › API controls › Manage App Access › Configure new app. Search for client ID `781568400878-iqlns1buart5fht4464a9o483aq0h89v.apps.googleusercontent.com` and choose **Trusted**.
    *   Heron can only explain this, through the [§5a](#5a-failure-messages) message.
    *   While the client is in Testing, the work address must also be on the test-user list.

### 12. Microsoft, when a client exists

No Microsoft client is registered. The code above already handles several Microsoft accounts. This section is what registering one takes, and what changes then.

*   **Where to register.**
    *   Microsoft's quickstart lists an Azure account and a tenant as prerequisites. Two things are unverified ([fact M5](research/heron-multi-account-provider-facts.md)): whether a personal Microsoft account with no directory can register an app at all, and whether registering is free. So registering may not be possible at zero cost unless a tenant already exists.
    *   The likely place is the owner's work tenant, if its policy lets users register apps.
*   **Registration settings for one client that serves both personal and work accounts:**
    *   supported account types "Accounts in any organizational directory and personal Microsoft accounts" (`AzureADandPersonalMicrosoftAccount`), so the `/common` authority Heron uses works ([fact M1](research/heron-multi-account-provider-facts.md));
    *   the platform "Mobile and desktop applications", with redirect URI `http://localhost`. The port is ignored for `localhost`, and Heron already uses `localhost` for Microsoft (`src/server.rs:1399`);
    *   "Allow public client flows" turned on, and no client secret;
    *   the delegated permissions `openid`, `offline_access`, `User.Read`, `Mail.Read` and `Calendars.Read`, matching `Provider::scopes`.
*   **A single-tenant registration.** This is the Entra default for a new app.
    *   `/common` then fails with AADSTS50194, which Heron reports with the generic Microsoft message and its code.
    *   The change that registers such a client also adds an optional build-time tenant (for example `HERON_MICROSOFT_OAUTH_TENANT`, default `common`), which `authorize_url` and `token_url` use. It isn't built now, because no tenant exists to test it against.
    *   With a tenant set, only that tenant's accounts can sign in, and personal Microsoft mail stays on "Use your Mac's Mail".
*   **Build.** The same change passes the client ID, and any tenant, through `packaging/macos/build-local-with-google.sh` the same way as Google's: from the environment, never committed or put in CI. A Microsoft public client has no secret to keep.
*   **Admin blocks.**
    *   Apps registered with a personal account can't be publisher-verified, and verification also needs a Microsoft AI Cloud Partner Program ID ([fact M4](research/heron-multi-account-provider-facts.md)).
    *   So tenants that allow consent only to verified publishers, or that use risk-based step-up consent, need an admin to grant consent. The [§5a](#5a-failure-messages) messages cover those codes.
    *   In the owner's own work tenant, an admin can grant tenant-wide consent once, under Enterprise applications › Heron › Permissions.
*   **Changing the client later.** Microsoft's `sub` depends on the client ID and the tenant ([fact M2](research/heron-multi-account-provider-facts.md)), and refresh tokens belong to the client they were issued to.
    *   After a change of client ID, every Microsoft account's refresh fails, and each one is signed out.
    *   Signing in under the new client gives a new `sub`, so the account arrives as a new row. The owner then disconnects the old rows.
    *   That is acceptable for a one-time change. Matching on `oid`+`tid` across clients is under [Future Improvements](#future-improvements).

## Alternatives Considered

### Alternative A: Add an `account` column to `knowledge` and `memories`

*   **Pros**:
    *   Source names stay `google_email`, so every exact-match check, taint list and alias list works unchanged.
    *   The account is a typed field, not part of a string.
*   **Cons**:
    *   `UNIQUE(source, ref)` would have to become `UNIQUE(source, account, ref)`. SQLite can't change a constraint in place, so the migration would rebuild `knowledge` together with its external-content full-text search table and its three triggers.
    *   Every store function that takes a source would gain an account parameter, and every caller would change: `replace_knowledge_snapshot`, `clear_knowledge`, `knowledge_of`, `knowledge_count`, `search_knowledge_in`, `delete_memories_by_source`, `memories_by_source`, and `Memory::save_detailed`.
    *   It needs a schema version step (`to_v4`) that fills `account` for existing rows. But the account ID depends on the `sub` held in the vault, which `store::migrate` can't read, so the rows would still need a second migration in `oauth.rs`.
*   **Reason for Rejection**: It changes more code, and its migration has the same vault dependency, so it isn't simpler. The chosen design's weak point is an exact-match check that misses a suffixed name. That is contained: `OUTSIDE_SOURCES` becomes private, and test I-4 plus a grep check cover the two checks that remain.

### Alternative B: The first account keeps the bare source names

*   **Pros**: There is no data migration; only new accounts get suffixed sources.
*   **Cons**:
    *   Two naming rules would live side by side forever.
    *   Disconnecting the first account and keeping the second would leave the second with suffixed names and the next one with bare names.
    *   Per-account code would branch on "first or not" everywhere.
*   **Reason for Rejection**: It is a permanent special case to avoid a migration that runs once.

### Alternative C: Random account IDs, with `sub` stored in each record

*   **Pros**: The ID shows nothing derived from `sub`.
*   **Cons**:
    *   Finding a duplicate means reading every record, and records would need a `sub` field again.
    *   A migration that stops halfway and runs again could create a second ID for the same account and split its data, unless the chosen ID is saved first. That adds another step to order.
*   **Reason for Rejection**: An ID that is the same every time means duplicate detection and re-runs need no extra lookup or step. A truncated hash of a non-secret identifier reveals nothing useful.

### Alternative D: One sync lock per provider, as today

*   **Pros**: There is no lock map.
*   **Cons**: Two accounts of one provider would sync one after the other, so one slow account would hold up the others. Safety doesn't need a provider lock, because tokens and epochs are already per account.
*   **Reason for Rejection**: The per-account lock is the unit that protects a rotating refresh token. A provider lock adds waiting and no safety.

### Alternative E: Keep a `Legacy` runtime path for unmigrated connections

*   **Pros**:
    *   If the migration failed at start-up, the old single account would keep syncing under its old entry and source names.
    *   Connections with no recorded `sub`, or with a token only in the fallback file, would need no special filing.
*   **Cons**:
    *   It means a second naming rule, bare source names, an extra admission case, a `"legacy"` route ID and its own tests, kept forever.
    *   The cases it protects are rare:
        *   a fallback-file token already can't sync (`src/oauth.rs:298-299`);
        *   a connection with no `sub` can't be signed in again today (`src/oauth.rs:235`), and its seven-day authorization runs out;
        *   a failed migration is retried at every start.
*   **Reason for Rejection**: The cost is permanent, and the benefit is one start-up's worth of sync after a rare failure. The design files every old connection under one naming rule ([§8](#8-migrating-todays-single-account)), and accepts that a provider whose upgrade is incomplete doesn't sync until it completes.

### Alternative F: Build the read-side source lists from the registry

*   **Pros**: Only listed accounts' sources are ever named.
*   **Cons**:
    *   `connected_summary`, `search` and `upcoming` take only the store today (`src/sources.rs:250`, `:602`, `:626`). They would need the config, and a vault read per account, on every system prompt and tool call.
    *   Disconnect already deletes an account's items, so the registry adds nothing for reads.
*   **Reason for Rejection**: A store-level family match does the same job with no vault reads ([§3](#3-per-account-sources-with-taint-unchanged)). Only `learn` uses the registry, because it needs each account's epoch.

### Alternative G: One secret-store entry per field, as today

*   **Pros**:
    *   It follows the existing pattern: `oauth_<p>_<field>`, written with `store_secret` (`src/oauth.rs:265-276`).
    *   Each field can be copied on its own during migration.
*   **Cons**:
    *   Six entries plus a registry are written one at a time. That creates partial states: a token whose epoch was lost, a listed account without a subject, a token that nothing lists.
    *   `store_secret` can fall back to the plaintext file, while `secret()` reads the vault first (`src/config.rs:688-702`). So a fallen-back write can stay invisible behind an old vault value.
    *   `secret()` maps any keyring read error to "absent". Combined with read-modify-write, a passing Keychain error could empty the registry, or overwrite a newer token.
*   **Reason for Rejection**: One vault-only record per account makes install, renewal and disconnect one verified write each. It removes the partial states and gives read errors a defined meaning ([§2](#2-secret-layout-one-vault-record-per-account)). The stage 5 review proposed this, and the stage 7 design review showed the failure cases.

--------------------------------------------------------------------------------

## Non-Functional Requirements

### Security invariants (rule Q7)

The owner asked for rule Q7 review of this change. It touches:

*   the credential code (`src/oauth.rs`, `src/config.rs`);
*   the taint-adjacent memory checks (`src/memory.rs`);
*   the server's OAuth routes (`src/server.rs`).

These invariants are what the review checks. Each has a named test, and U1's check requires every test listed here and in the [Verification Plan](#verification-plan).

| id | Invariant | Test (in `src/oauth.rs` unless noted) |
|---|---|---|
| I-1 | A sign-in for `sub` S writes only `id(S)`'s record and the registry. It never writes another account's record or sources. | `i1_a_sign_in_writes_only_its_own_account` |
| I-2 | A sync or learning pass that captured account A's epoch commits nothing after A is disconnected or signed in again. It never writes into account B's sources. | `i2_a_sync_that_outlives_its_account_writes_nothing`, `i2_learning_stops_when_its_account_changes` (`src/sources.rs`) |
| I-3 | Disconnecting A removes exactly A's record, items, memories and registry entry. B is unchanged, even when a vault read fails during the disconnect. | `i3_disconnect_removes_one_account_and_keeps_the_other`, `i3_a_vault_read_error_never_empties_the_registry` |
| I-4 | Every per-account source is outside content. Its memories start unconfirmed, a model save that echoes one stays unconfirmed, and a suffixed name is never trusted. | `i4_every_account_source_counts_as_outside` (`src/memory.rs`) |
| I-5 | The migration never overwrites an existing record, never deletes the old token before the rename commits, can run again from any step, and never brings back an account the owner disconnected during an incomplete upgrade. | `i5_migration_can_stop_after_any_step_and_run_again`, `i5_migration_never_overwrites_an_existing_record`, `i5_disconnect_during_an_incomplete_upgrade_stays_disconnected` |
| I-6 | Text the provider sends in the callback (`error_description`) is never rendered, logged or returned. Only fixed messages and `AADSTS` digits are. | `i6_provider_error_text_is_never_shown` (`src/server.rs`) |
| I-7 | No refresh token, `sub` or epoch appears in any API response. | `i7_oauth_status_lists_accounts_without_secrets` (`src/server.rs`) |
| I-8 | Two accounts of one provider sync at once, and each refresh saves only its own rotated token. | `i8_two_accounts_sync_at_once_with_their_own_tokens` |
| I-9 | A fact learned from account A never updates or retires a memory from account B. | `i9_a_fact_seen_in_two_accounts_stays_in_each` (`src/memory.rs`) |

### Risks and mitigations

| Risk | Impact | Mitigation | Where |
|---|---|---|---|
| A failed migration leaves a provider not syncing | Medium: no new mail until a start succeeds | Retried at every start. Settings shows the upgrade notice. Other providers are unaffected. | §8 |
| The migration runs on the owner's live `heron.db` | High if it went wrong | A one-time `before-multi-account` backup before any change. The rename runs in one SQLite transaction. | §8 |
| Backups keep mail after Disconnect | Medium: privacy | The migration makes one `before-multi-account` copy and never another. The PR and the README say Disconnect doesn't purge backups. | §6, §8 |
| A work mailbox becomes searchable through Heron's MCP server | Medium: may break employer policy | Owner call Q-3. A per-account switch is a Future Improvement. | Q-3 |
| The Slint app can add a second account it can't disconnect | Low | 409 message points to the Mac app. Non-goal. | §9 |
| A disconnect cancels an unrelated sign-in in progress | Low: start again | Message says so. | §6 |
| Google sign-ins in Testing mode end every 7 days | Medium: weekly click per account | One-click Sign in again (G-6), or Q-2. | §7, §11 |
| Whether providers redirect on admin or test-user blocks is unverified (facts G5, M4) | Medium: the app may only see a timeout | The hint shows from the start of the wait. | §5a |
| A second Heron process running during the upgrade (for example the CLI) | Low | Only the Heron that first gets the background lock migrates, including on a takeover. Others show `upgrade_pending` until it is done. | §8 |
| Microsoft registration may not be possible at zero cost (fact M5, unverified) | Microsoft OAuth stays unavailable | Mac Mail covers personal Microsoft mail. §12 is a plan only. | §12 |

### Performance

*   Background sync grows linearly with accounts: 28 Google requests per account every 30 minutes, at most five accounts per provider.
*   The migration is one `UPDATE` per base over the existing rows, which is fewer than 100 rows in the owner's database today.
*   The read paths (prompt summary, search, upcoming events) make no vault reads.

### Rollout and rollback

*   **Rollout.** A draft PR into `main`, held until trial G9's PR has merged. It needs the owner's rule Q7 review. There is no feature flag: the migration runs once, and a database with no OAuth account sees no change.
*   **Rollback.** Follow the Downgrade steps in [§8](#8-migrating-todays-single-account): disconnect each account in the new build, run `heron restore` with the `before-multi-account` copy, and sign in again under the old build.

--------------------------------------------------------------------------------

### Additional Resources

*   [Provider facts, checked 2026-11-22](research/heron-multi-account-provider-facts.md). Every external claim above cites it.
*   PR #72 (`fix/mac-connect-sources`, `614d681`): the Connections and first-run UI this design builds on.
*   [heron-v1-features.md §2, The features](heron-v1-features.md#2-the-features): the rule Q7 path list.
*   `packaging/macos/build-local-with-google.sh`: how the Google client ID and secret reach a local build.
*   Review reports: `docs/designs/reviews/heron-multi-account/`.

### Future Improvements

*   **Keep an account out of other apps.** A per-account switch that hides it from the MCP server's tools (Q-3).
*   **Microsoft `oid`+`tid` identity.** Asking for `profile` and keying Microsoft accounts on `oid@tid` would keep an account's data across a change of client ID ([§12](#12-microsoft-when-a-client-exists)). It only helps if the client ever changes.
*   **A Microsoft tenant setting and build passthrough.** They land with the change that registers a Microsoft client.
*   **A "Work" or "Personal" tag.** From Google's `hd` claim and Microsoft's `tid`. It is useful once two Microsoft accounts can share an address.
*   **Account addresses in search results.** For example `[Gmail · work@example.com]` instead of `google_email:<id>`, so the model can say which account an item came from.
*   **Multi-account rows in the Slint app.**
*   **Choosing which calendars or labels of an account to read.**
*   **De-duplicating a mailbox read through both OAuth and Mac Mail.**

--------------------------------------------------------------------------------


## Detailed Implementation Plan

### Target Files to Create/Modify

#### 1. `src/oauth.rs` (Modify)

*   **Type / Scope**: Rust core, credentials and sync.
*   **Proposed Changes**:
    *   **Types.**
        *   `AccountRef { provider: Provider, id: String }`.
        *   `AccountRecord { refresh_token: Option<String>, client_id: Option<String>, epoch: String, label: Option<String>, needs_sign_in: bool }`, with `serde`. `AccountRecord::connected()` is true when `refresh_token` and `client_id` are both present and `needs_sign_in` is false. The API's `needs_sign_in` is `!connected()`, and a listed account with no readable record is not connected.
        *   `enum Admission { Added, Renewed }`.
        *   `enum SignInError`: the closed list in [§5a](#5a-failure-messages), plus `Other` for the generic message. It has `fn message(&self) -> String`.
        *   `TokenError { status: u16, error: Option<String> }`, which `redeem` returns. It holds nothing from the response body except the `error` value.
        *   `enum ProviderHttp { Live, #[cfg(test)] Fake(Arc<FakeProvider>) }`, with `#[cfg(test)] pub(crate) struct FakeProvider` at module level, so server tests can build one ([Test seams](#test-seams)).
        *   `const MAX_ACCOUNTS_PER_PROVIDER: usize = 5`.
    *   **Identity and records.**
        *   `fn account_id(provider, sub) -> String` ([§1](#1-account-identity)).
        *   `AccountRef::record_key()`, `mail_source()` and `calendar_source()`.
        *   `fn read_record(cfg, &AccountRef) -> Result<Option<AccountRecord>>`. It uses a verified read. A record that doesn't parse returns a distinct `RecordError::Unreadable`. Every caller treats that as "no record", as [§2](#2-secret-layout-one-vault-record-per-account) says. Migration step 5 also leaves the record alone.
        *   `fn write_record(cfg, &AccountRef, &AccountRecord) -> Result<()>`, through `store_vault_only_secret`.
        *   `fn read_registry(cfg, provider) -> Result<Vec<String>>`, a verified read.
        *   `fn registry_add` and `fn registry_remove`. Their callers hold `SOURCE_WRITES`.
        *   `fn accounts(cfg, provider) -> Result<Vec<AccountSummary>>`, where `AccountSummary { account: AccountRef, label, connected, needs_sign_in }`. `account.id` is serialised as `id`.
        *   `fn any_listed(cfg, provider) -> Result<bool>` and `fn upgrade_pending(cfg, provider) -> Result<bool>`.
        *   `fn legacy_entry(cfg, provider) -> Result<Option<LegacyEntry>>` ([§8](#8-migrating-todays-single-account), step 1).
        *   The epoch uses the existing `random_secret()` (`src/oauth.rs:103`).
        *   `fn bundled_client_id(provider) -> Option<&'static str>` wraps the `option_env!` reads. Under `#[cfg(test)]`, an enrolled home can set an override, so tests can cover the "bundled ID" branches of the client-ID order and of `client_configured`.
    *   **Sign-in.**
        *   `begin` gains `login_hint: Option<&str>` and the `prompt` rules ([§5](#5-sign-in-and-its-reported-result)). `authorize_url` and `token_url` stay `&'static str`.
        *   `finish_with(cfg, http, pending, code) -> Result<SignInResult, SignInError>`, where `SignInResult { account, label, outcome: Admission }`. The requested account ID for Sign in again travels in `Pending`. The public `finish` passes `ProviderHttp::Live`.
        *   `install(cfg, provider, client_id, token, requested: Option<&str>) -> Result<SignInResult, SignInError>` applies the admission table in [§5](#5-sign-in-and-its-reported-result) under `SOURCE_WRITES`.
        *   `admit(requested_differs: bool, listed: bool, count: usize) -> Result<Admission, SignInError>` is a pure function that returns the four rows of that table.
    *   **Per-account functions.**
        *   `get_json(http, token, url, params)` and the four fetchers (`google_mail`, `google_calendar`, `microsoft_mail`, `microsoft_calendar`) take `&ProviderHttp` instead of `&reqwest::Client`. `Live` builds the client with the existing `oauth_http()`.
        *   `access_token_with_generation(cfg, http, &AccountRef)` reads the record with a verified read, checks it before saving a rotated token, and sets `needs_sign_in` under the guard in [§7](#7-signed-out-after-expiry-or-revocation).
        *   `sync_with(app, http, &AccountRef)`. The public `sync(app, &AccountRef)` passes `Live`.
        *   `disconnect(app, &AccountRef)` follows the order in [§6](#6-disconnect-one-account). That includes step 4, which removes the old token, the old entries, and the bare-named items and memories when the ID is the migration's target.
        *   `disconnect_old_data(app, provider)` does the same removal, including the target ID's per-account sources, for `DELETE /api/oauth/{p}` when no account is listed.
    *   **Locks.** `GOOGLE_SYNC` and `MICROSOFT_SYNC` are replaced by `static SYNC_LOCKS: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>`, keyed by `<provider>:<id>`.
    *   **Migration.** `pub fn migrate_legacy_accounts(app: &App)` logs failures and returns nothing. It runs steps 1 to 7 of [§8](#8-migrating-todays-single-account) in order:
        1.  read the old entries;
        2.  back up the database, unless a copy already exists;
        3.  rename;
        4.  add the ID to the registry;
        5.  write the record;
        6.  delete the old token, which is the commit point;
        7.  clear the other old entries.
    *   **Messages.** `pub fn describe_failure(provider, error: Option<&str>, description: Option<&str>) -> String`. It reads the `AADSTS` digits with `regex` (`AADSTS(\d{5,7})`).
    *   **Deleted.**
        *   `enum Admit` and the old `admit` (`:223-237`).
        *   `Provider::connected`, `Provider::account_epoch`, `Provider::mail_source`, `Provider::calendar_source` (`:69-80`).
        *   `account_name` (`:219`), `access_token` (`:327`) and `account_subject`'s callers outside sign-in.
        *   `Provider::secret_key` stays, renamed `legacy_key`, for `legacy_entry` only.

        Clippy with `-D warnings` finds any remaining caller.
    *   **Existing tests.**
        *   `the_bundled_google_secret_goes_only_to_google_with_the_bundled_client_id`, `the_account_label_is_the_email_claim_or_the_microsoft_username`, `a_refresh_request_carries_the_secret_the_same_way`, `the_signed_in_account_comes_from_an_id_token_issued_to_this_app`, `incomplete_granular_google_consent_is_not_accepted_as_full_sync` and `pkce_is_sha256_base64url_without_padding` stay as they are.
        *   `callbacks_are_only_local_and_bound_to_the_provider` and `read_only_authorization_scopes_and_unique_state` pass `None` for the new `login_hint` argument.
        *   `only_google_sign_ins_must_show_both_gmail_and_calendar_consent` calls the new `install` with `requested: None`. Its error-string checks stay.
        *   `only_the_connected_account_may_sign_in_again_without_disconnecting` is deleted. `admission_table_rows` replaces it.
        *   `a_failed_reconnect_keeps_the_client_id_the_old_token_belongs_to` becomes `a_failed_record_write_keeps_the_old_entries`, which runs on every OS through the test vault.
        *   `a_failed_disconnect_keeps_the_account_listed_so_it_can_be_retried` becomes `migration_files_a_fallback_file_token_as_signed_out`, which then disconnects and checks that the items are gone.
        *   `reconnecting_the_same_account_keeps_its_data_vault_live` keeps its exact name, because CI's vault step runs it with `--exact` (`.github/workflows/ci.yml:34`). It is rewritten for per-account records, and it now checks that renewal writes a *new* epoch while items and memories stay.
    *   **New tests.** Every `src/oauth.rs` test named under [Security invariants](#security-invariants-rule-q7) and in the [Verification Plan](#verification-plan).

#### 2. `src/config.rs` (Modify)

*   **Type / Scope**: Rust core, secrets.
*   **Proposed Changes**:
    *   **`pub fn vault_only_secret_checked(&self, name) -> Result<Option<String>>`.** It wraps `vault::get_checked`. It returns `Ok(None)` on a platform with no vault when the home isn't enrolled in the test vault. On error it returns the fixed Keychain message and logs the OS text ([§2](#2-secret-layout-one-vault-record-per-account)).
    *   **`pub fn fallback_file_secret(&self, name) -> Result<Option<String>>`.** It reads the plaintext fallback file only, through the existing private `read_file_secrets` (`:629`).
    *   **The `#[cfg(test)]` test vault** from [Test seams](#test-seams): `enroll_test_vault()` and a failure switch, `fail_next(VaultOp)`, where `VaultOp` is `Read`, `Write` or `Delete`.
        *   It is wired into **both** `vault` modules: the macOS and Windows one (`:878-922`) and the one for other platforms (`:924-945`).
        *   It is checked in `get`, `set`, `set_verified`, `get_checked` and `delete`, and in the OS checks of `store_vault_only_secret` (`:718-720`) and `delete_vault_only_secret` (`:742-746`). It is also checked in `start_oauth`'s OS check (`src/server.rs:1379-1382`). Under `#[cfg(test)]`, that check passes when `cfg.home` is enrolled, so the server sign-in tests run on the Linux CI runner. Release builds keep the 501.
        *   So an enrolled home always takes the verified write and delete paths.
        *   Release builds compile none of it.
    *   **Test** `a_checked_read_of_a_missing_entry_is_none`, with the same assertion on every OS.

#### 3. `src/sources.rs` (Modify)

*   **Type / Scope**: Rust core, sources and learning.
*   **Proposed Changes**:
    *   **Helpers.** `pub fn base_of(source) -> &str`, and `pub fn account_of_source(source) -> Option<AccountRef>`. The second accepts only an OAuth base followed by `:` and 32 lowercase hex characters. `SOURCES` keeps the bare names.
    *   **`learn`.** It gets the connected accounts from `oauth::accounts`, and learns from each account's two sources. It leaves out the bare OAuth bases. On a vault read error it skips the OAuth sources, logs the error, and still learns from the others.
    *   **`learn_one`.** It accepts a name in `SOURCES` other than the four bare OAuth bases, or a name that `account_of_source` accepts. It uses that account's epoch.
    *   **Reads.** `search`, `upcoming` and `connected_summary` use the store's family queries (item 8), as [§3](#3-per-account-sources-with-taint-unchanged) describes. `source_label` and the upcoming-events label match on `base_of`.
    *   **`sync_due`.** It runs `oauth::sync` for each connected account in turn, and skips OAuth for that cycle on a vault read error.
    *   **Tests.**
        *   New: `i2_learning_stops_when_its_account_changes` and `learn_one_refuses_a_bare_oauth_base`.
        *   Updated: the search, upcoming and summary tests (`:850-870`) use two Google accounts' per-account names.
        *   `learning_from_email_reports_items_as_waiting` (`:789-803`) must still pass unchanged on Linux.

#### 4. `src/memory.rs` (Modify)

*   **Type / Scope**: Rust core, memory trust.
*   **Proposed Changes**:
    *   `OUTSIDE_SOURCES` becomes private: `const`, not `pub const`.
    *   `pub fn is_outside(source: &str) -> bool` is used at `:278` and `:369`. Its body holds the only `OUTSIDE_SOURCES.contains` in `src/`.
    *   **Near-match exclusion.** In `save_detailed`, the rule from [§3](#3-per-account-sources-with-taint-unchanged) applies on the vector scan (`:270-277`), the outside-row scan (`:288-300`) and the `replaces` target (`:333-337`). It uses `sources::account_of_source`.
    *   **Tests.**
        *   `i4_every_account_source_counts_as_outside`. For each per-account source name: a learned memory is unconfirmed, an `agent` save of the same text stays unconfirmed, and `TRUSTED_SOURCES` rejects the name.
        *   `i9_a_fact_seen_in_two_accounts_stays_in_each`, with one case for each of the three paths.

#### 5. `src/app.rs` (Modify)

*   **Type / Scope**: Rust core, start-up.
*   **Proposed Changes**:
    *   `App::own_background_work()` calls `oauth::migrate_legacy_accounts(self)` when it first gets the lock, next to `recover_interrupted_approvals` and `recover_interrupted_jobs` (`src/app.rs:2112-2114`). `src/main.rs` and `server::serve` need no change.
    *   The tests that call `own_background_work` (`src/app.rs:3375-3425`) have no old entry, so the migration is a single vault read for them. On macOS that is a read of a Keychain entry that doesn't exist, as today's tests already do.
    *   The `src/app.rs:3622` test uses a per-account source name.

#### 6. `src/server.rs` (Modify)

*   **Type / Scope**: Rust core, HTTP API.
*   **Proposed Changes**:
    *   **Routes**, per [§9](#9-http-api):
        *   the new account routes;
        *   the sign-in poll route;
        *   the provider-level sync and delete routes, kept for the Slint app;
        *   the new `get_oauth_status` shape (`max_accounts`, `upgrade_pending`, `accounts`, the new rule for `client_configured`), and HTTP 503 on a vault read error;
        *   the `start_oauth` client-ID order and `account_id`, reading the bundled ID through `oauth::bundled_client_id` instead of `option_env!` directly (`:1361`, `:1388`);
        *   the `start_oauth` OS check, which an enrolled test home passes ([item 2](#2-srcconfigrs-modify));
        *   `get_sources` counts through the family queries.
    *   **`disconnect_source`.**
        *   The check at `:1593`, for the legacy `email` and `calendar` sources, becomes "any listed account of either provider" (`oauth::any_listed`).
        *   Bare OAuth names are refused only while that provider's `upgrade_pending` is true.
        *   Read errors are returned.
    *   **State.**
        *   `oauth_pending` becomes the sign-in record map, with its own `std::sync::Mutex` and the states in [§5](#5-sign-in-and-its-reported-result). The `attempt` handle uses the existing `new_token()` (`:91`).
        *   `oauth_http: ProviderHttp` is added.
    *   **Test seam.** `#[cfg(test)] fn router_with_http(app, token, loopback_only, http) -> Router`. `router_with` calls it with `Live`.
    *   **Callback.** It marks the sign-in record as exchanging (internal only), then sets its result. It maps errors through `describe_failure` and `SignInError::message`, spawns the first sync for that one account, and sets `first_sync` when that sync ends.
    *   **Test harness** (`:1693-1798`):
        *   a `Host` header with a port (`127.0.0.1:8080`);
        *   a `send_raw(method, full_path)` for paths outside `/api`, so tests can call the callback;
        *   server OAuth tests enrol the test vault in the harness's config set-up and build the router with `router_with_http(…, ProviderHttp::Fake(…))`.
    *   **Existing tests.**
        *   Any existing server test that reads the old `/api/oauth` shape is rewritten to the [§9](#9-http-api) literals.
        *   The `:1712` assertion uses `memory::is_outside`.
    *   **New tests:**
        *   `i6_provider_error_text_is_never_shown`;
        *   `i7_oauth_status_lists_accounts_without_secrets`, which asserts the exact key sets of the §9 literals;
        *   `provider_level_delete_refuses_when_two_accounts_exist`;
        *   `provider_level_sync_and_delete_with_zero_and_one_account`;
        *   `sign_in_records_report_added_renewed_and_failed`, which asserts the §9 sign-in key set;
        *   `start_picks_the_client_id_in_order`;
        *   `start_with_an_unknown_account_id_is_404`;
        *   `the_sign_in_map_refuses_a_seventeenth_record`;
        *   `disconnect_fails_waiting_sign_ins_with_a_message`;
        *   `disconnect_source_refuses_legacy_email_while_a_signed_out_account_is_listed`;
        *   `oauth_status_reports_upgrade_pending_and_client_configured`;
        *   `bare_rows_can_be_erased_once_the_migration_is_done`, which also checks the refusal while `upgrade_pending`.

#### 7. `src/bin/desktop/setup_glue.rs` (No change)

*   **Type / Scope**: Slint app client.
*   **Proposed Changes**: None, because:
    *   `apply_oauth_status` reads only each provider's `connected` (`:991-998`), which keeps its meaning;
    *   a refused `DELETE /api/oauth/{p}` already shows its error text (`:1105-1107`);
    *   a failed `GET /api/oauth` is already ignored (`:1009`).

    `cargo test --locked --features desktop --bin heron-desktop` in the `verify` unit confirms it still builds and passes.

#### 8. `src/store.rs` (Modify)

*   **Type / Scope**: Rust core, store.
*   **Proposed Changes**:
    *   **`rename_sources(&self, pairs: &[(&str, &str)]) -> Result<(usize, usize)>`.** In one transaction, for every pair, it:
        *   renames `knowledge` rows, dropping a bare row whose (new source, ref) already exists;
        *   renames `memories` rows;
        *   sets `memories.tags` to the new name where it equals the old one.

        It returns `(knowledge rows renamed, memory rows renamed)`.
    *   **Family queries.**
        *   New functions: `knowledge_count_family(base)`, `search_knowledge_families(expr, bases, limit)` and `knowledge_between_family(base, from, to, limit)`.
        *   Each matches `source = base` or `substr(source, 1, length(base) + 1) = base || ':'`.
        *   The existing exact-match functions stay.
    *   **Migration backup.** `pub fn backup_before_multi_account(&self) -> Result<Option<PathBuf>>`.
        *   It returns `None`, and does nothing, for an in-memory store, or when a usable `before-multi-account` copy already exists in `backup_dir`. Usable means it opens read-only and `damage` finds nothing.
        *   Otherwise it calls `snapshot(conn, path, "before-multi-account")`.
        *   It never prunes that label.
    *   **No schema change.** There is no new table, column or version.
    *   **Tests**, in `store::tests` (`:1714`):
        *   `rename_sources_moves_rows_and_drops_duplicates`;
        *   `family_queries_match_bare_and_suffixed_names_only`, which checks that `google_emailx` and `google_email_x` don't match the `google_email` family;
        *   `the_first_migration_backup_survives_a_retry`, which backs up, runs again with the copy's file time set 11 minutes back, and checks that the first copy is still there and no second one was made. A second case plants an empty `heron-before-multi-account-*.db` and checks that a real copy is still made.

#### 9. `macos/HeronMac/Sources/HeronCore/{Models,Wire,CoreClient,CoreClientProtocol,ManagedCoreClient,PreviewCoreClient,PreviewData}.swift` (Modify)

*   **Type / Scope**: Swift core client.
*   **Proposed Changes**:
    *   **Models.**
        *   `OAuthProviderStatus { provider, clientConfigured, readOnly, connected, maxAccounts, upgradePending, accounts: [OAuthAccount] }`.
        *   `OAuthAccount { id, label: String?, connected, needsSignIn }`.
        *   `SignInStart { url, attempt }`.
        *   `SignInStatus { status, accountID?, label?, message?, firstSync?, firstSyncMessage? }`, where `status` is `waiting`, `added`, `renewed`, `failed` or `expired`.
        *   `DisconnectResult { removedItems, removedMemories }`.
    *   **Wire decoders** use transfer structs. Under `convertFromSnakeCase`, `account_id` would become `accountId`, so the structs map it to `accountID` explicitly. The https-only check on `authorization_url` stays.
    *   **Client methods.**
        *   `oauthProviders()`;
        *   `startOAuth(provider:accountID:) -> SignInStart`;
        *   `signInStatus(attempt:) -> SignInStatus`;
        *   `disconnectOAuth(provider:accountID:) -> DisconnectResult`.

        `syncOAuth` is removed, because the Mac app no longer calls sync after a sign-in.
    *   **`PreviewCoreClient`** simulates several accounts, each sign-in status, a failed first sync, the cap and `upgradePending`. Its test controls are replaced:
        *   `completeSignIn(afterPolls:address:)` becomes `completeSignIn(afterPolls:address:outcome:)`, where `outcome` is `.added`, `.renewed` or `.failed(message)`;
        *   `failNextOAuthSync` becomes `failNextFirstSync(with:)`;
        *   `oauthSyncs` is removed;
        *   `setOAuthConfigured` and `failNextOAuthStart` keep their meaning.
    *   **In `SettingsStore`** (`SettingsAndFirstRunStores.swift:180`), `account(_:)` is replaced by:
        *   `provider(_:) -> OAuthProviderStatus`, with today's fallback when the core didn't list the provider (no accounts, `clientConfigured: true`, `maxAccounts: 5`);
        *   `accounts(_:) -> [OAuthAccount]`.

        Their callers move to them: `ConnectionsViews.swift:100`, `ScreenStoreTests.swift:625-626` and `ConnectionsSnapshotTests.swift:31,39`.

#### 10. `macos/HeronMac/Sources/HeronUI/Stores/SettingsAndFirstRunStores.swift` and `Views/ConnectionsViews.swift` (Modify)

*   **Type / Scope**: Swift UI.
*   **Proposed Changes**:
    *   **Store.**
        *   Polling uses `signInStatus` until `status` is no longer `waiting`, then until `firstSync` is no longer `running`.
        *   Cancel stops polling and clears the waiting state, and makes no server call.
        *   Busy state, error state, `PendingDisconnect` and its dispatch are keyed per account, as [§10](#10-mac-app) says.
    *   **Views.** Add `ProviderSection`, the account rows and the upgrade notice. `SourceCopy` gains the strings quoted in [§5a](#5a-failure-messages), [§8](#8-migrating-todays-single-account) and [§10](#10-mac-app).

#### 11. `macos/HeronMac/Tests/…` (Modify)

*   **Type / Scope**: Swift tests.
*   **Proposed Changes**: Test methods follow the existing `testCamelCase` style, and the U2 check greps those exact names.
    *   **New in `ScreenStoreTests`:**
        *   `testAddsASecondGoogleAccount`;
        *   `testAddingAListedAccountShowsRenewed`;
        *   `testDisconnectRemovesOnlyThatAccount`;
        *   `testFailedSignInShowsTheCoreMessage`;
        *   `testFailedFirstSyncShowsTheMessage`;
        *   `testCapDisablesAddAnotherAccount`;
        *   `testSignInAgainSendsTheAccountID`;
        *   `testCancelStopsWaitingLocally`;
        *   `testUpgradePendingShowsTheNotice`.
    *   **Rewritten in `ScreenStoreTests`.** The five existing OAuth tests (`:577-669`) are rewritten to the new flow:
        *   `oauthSyncs` checks become first-sync checks;
        *   `failNextOAuthSync` becomes `failNextFirstSync`;
        *   `pendingDisconnect?.id == "google"` becomes a match on `.account(.google, _)`.
    *   **`ModelDecodingTests` and `CoreClientTests`.**
        *   `testOAuthAccountsDecodeProvidersAndSkipUnknownOnes` (`ModelDecodingTests.swift:103-108`) becomes `testOAuthProvidersDecodeTheContractLiterals`, which decodes the exact [§9](#9-http-api) literals.
        *   `testOAuthRoutes` (`CoreClientTests.swift:196-219`) checks the new paths.
    *   **`ConnectionsSnapshotTests`.**
        *   `testConnectionsTabWithTwoGoogleAccountsLight` is new.
        *   The existing tests (`:29-56`) use the new preview controls.
        *   Their reference snapshots are re-recorded: delete the PNGs, run `swift test` once to record them, then run it again. The owner reviews the new PNGs under rule Q7.

#### 12. `packaging/macos/README-google-signin.md` (Create)

*   **Type / Scope**: Owner docs.
*   **Proposed Changes**:
    *   The owner steps from [§11](#11-google-test-users-and-workspace-blocks): test users, the weekly sign-in, and the Workspace trust step.
    *   The registration plan in [§12](#12-microsoft-when-a-client-exists).
    *   A note that Disconnect doesn't purge `~/.heron/backups/`, and that the `before-multi-account` copy is meant only for the downgrade steps in [§8](#8-migrating-todays-single-account).
    *   No secrets go in the README, and `build-local-with-google.sh` is unchanged.

### Test seams

Today neither the vault nor the provider endpoints can be faked:
*   `store_vault_only_secret` refuses on Linux and reaches the real Keychain on macOS, which is why the existing round-trip test is `#[ignore]` (`src/oauth.rs:590-594`);
*   `redeem` and the mail and calendar reads make real HTTPS calls.

These seams let the tests above run in `cargo test --locked` on every OS. None is compiled into a release build except the `ProviderHttp::Live` value.

*   **Test vault** (`src/config.rs`, `#[cfg(test)]`).
    *   A process-wide in-memory map keyed by (Heron home folder, entry name), and a set of *enrolled* home folders, each with a failure switch (`fail_next(VaultOp)`) that fails the next read, write or delete.
    *   `Config::enroll_test_vault(&self)` enrols `self.home`. Each test uses its own temporary home, so tests running in parallel don't share entries, and clones of a `Config` share one vault through their home.
    *   Both `vault` modules check enrolment first ([item 2](#2-srcconfigrs-modify)). A home that isn't enrolled behaves as today, so the live Keychain tests in CI's vault step still reach the real vault.
*   **Provider HTTP** (`src/oauth.rs`).
    *   `FakeProvider` returns scripted token replies, keyed by the refresh token or code sent, as an HTTP status plus JSON. It returns scripted API JSON keyed by (access token, URL).
    *   So `i8_two_accounts_sync_at_once_with_their_own_tokens` can give each account its own replies.
    *   It can hold a call until the test releases it, with `tokio::sync::Notify`. That is how three tests pause a sync mid-fetch: `i2_a_sync_that_outlives_its_account_writes_nothing`, `i8_two_accounts_sync_at_once_with_their_own_tokens` and `a_stale_invalid_grant_never_signs_out_a_renewed_account`.
*   **Server.** `router_with_http` and the harness changes in [item 6](#6-srcserverrs-modify), and the enrolled-home pass of the `start_oauth` OS check.
*   **Bundled client ID.** `bundled_client_id`'s test override ([item 1](#1-srcoauthrs-modify)).
*   **Backup.** `backup_before_multi_account` returns `None` for an in-memory store, so migration tests can use `Store::in_memory()`.
*   **Stopping at each step.** `i5_migration_can_stop_after_any_step_and_run_again` stops the migration after each step, using the test vault's failure switch and a temporary `knowledge` trigger, as the existing disconnect test does (`src/oauth.rs:638`). It then runs the migration again and checks the final state.
*   **No test may be OS-limited or ignored.** No required test may be limited to one OS with `#[cfg(...)]` or marked `#[ignore]`. U1's check fails if any required name shows up as ignored.

### Verification Plan

*   **Build and lint:** `cargo clippy --locked --all-targets -- -D warnings`, and `cargo build --locked --features desktop --bins`.
*   **Rust tests.** `cargo test --locked` and `cargo test --locked --features desktop --bin heron-desktop`.
    *   Both run outside the agent sandbox. Inside it, about 27 existing tests fail, in cli_llm, mail, telegram, tools::shell, agent and some server tests: binding a local port returns EPERM there, and nested Seatbelt can't start (seen 2026-11-20).
*   **Named Rust tests that U1's check requires**, besides the invariant tests:
    *   In `src/oauth.rs`:
        *   `account_ids_are_stable_hashes_of_the_subject`, with the two vectors in [§1](#1-account-identity);
        *   `admission_table_rows`;
        *   `begin_sends_the_prompt_and_hint_for_each_request`;
        *   `adding_a_listed_account_renews_it_and_keeps_its_data`;
        *   `sign_in_again_as_a_different_account_is_refused`;
        *   `the_cap_refuses_a_new_account_but_not_a_renewal`;
        *   `an_expired_grant_signs_the_account_out`;
        *   `a_stale_invalid_grant_never_signs_out_a_renewed_account`;
        *   `a_failed_record_write_keeps_the_old_entries`;
        *   `migration_files_a_fallback_file_token_as_signed_out`;
        *   `migration_files_a_connection_without_a_subject_under_a_fixed_id`;
        *   `migration_stops_on_an_unreadable_old_subject`;
        *   `failure_codes_map_to_fixed_messages`.
    *   In `src/config.rs`: `a_checked_read_of_a_missing_entry_is_none`.
    *   In `src/sources.rs`: `learn_one_refuses_a_bare_oauth_base`.
    *   In `src/store.rs`: `rename_sources_moves_rows_and_drops_duplicates`, `family_queries_match_bare_and_suffixed_names_only` and `the_first_migration_backup_survives_a_retry`.
    *   In `src/server.rs`: the new tests in [item 6](#6-srcserverrs-modify).
*   **Swift tests:** `swift test --package-path macos/HeronMac`, run outside the agent sandbox on the `claude_build_swift` backend.
*   **Test quality (testteeth).**
    *   The command: `testteeth run --lang rust --base multi-account-base --fail-under 70 -q src/oauth.rs src/sources.rs src/memory.rs src/server.rs src/store.rs src/config.rs`.
    *   `multi-account-base` is a tag set on the commit the feature branch is cut from, before `skein init`. Only this change's functions are graded.
    *   The gate is the score: each surviving mutant is killed by a new test until the score is at least 70.
    *   Mutants that can't change behaviour are listed in the PR description. One example: removing an explicit `busy_timeout`, which rusqlite already sets to 5 s on every connection.
*   **genesis** doesn't apply: this change adds no evaluator.
*   **Manual, on the owner's Mac.** After trial G9, from an install branch that also carries every open fix PR the installed app already has (as of 2026-11-22: #70 and #71, on top of #72):
    1.  Run `heron backup`.
    2.  Install the build and check CUJ 4: the same Google account, with the item count unchanged.
    3.  Add the work Gmail (CUJ 1).
    4.  Add the personal Gmail again (CUJ 2).
    5.  Disconnect the work Gmail (CUJ 3), and confirm the personal account's item count didn't change.

### Work units

*   **U1 holds every Rust change.** All of them are under `src/**`. The per-account `oauth.rs` signatures break their callers in `server.rs`, `sources.rs` and `app.rs`, so a unit holding only one side would fail its check.
*   **U2 runs alongside U1.** It builds the Mac app against the [§9](#9-http-api) literals, through `PreviewCoreClient`.
*   **Checks.** Each check exits 0 only when its unit is done. Rust checks run outside the agent sandbox. Swift checks run on the `claude_build_swift` backend, by the owner's decision of 2026-11-19.

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| U1 | Every Rust change, plan items 1–8:<br>• `oauth.rs`: records, registry, admission, locks, epochs, signed-out flag, migration, `SignInError`, `describe_failure`, `ProviderHttp`, deletions;<br>• `config.rs`: checked reads, fallback-file read, test vault;<br>• `sources.rs`: names, learning, family reads;<br>• `memory.rs`: private `OUTSIDE_SOURCES`, `is_outside`, the near-match rule;<br>• `store.rs`: `rename_sources`, family queries, the one-time backup;<br>• `app.rs`: the migration hook;<br>• `server.rs`: routes, sign-in records, callback, shapes, test seam.<br>Spec: §1–§9 and Test seams | `src/**` | none | `cargo test --locked --lib && cargo clippy --locked --all-targets -- -D warnings && [ "$(git grep -c 'OUTSIDE_SOURCES.contains' -- src \| awk -F: '{s+=$2} END {print s+0}')" = 1 ] && git grep -q 'OUTSIDE_SOURCES.contains' -- src/memory.rs && ! git grep -q 'pub const OUTSIDE_SOURCES' -- src && L="$(cargo test --locked --lib -- --list 2>/dev/null)" && I="$(cargo test --locked --lib -- --list --ignored 2>/dev/null)" && for t in oauth::tests::i1_a_sign_in_writes_only_its_own_account oauth::tests::i2_a_sync_that_outlives_its_account_writes_nothing sources::tests::i2_learning_stops_when_its_account_changes oauth::tests::i3_disconnect_removes_one_account_and_keeps_the_other oauth::tests::i3_a_vault_read_error_never_empties_the_registry memory::tests::i4_every_account_source_counts_as_outside oauth::tests::i5_migration_can_stop_after_any_step_and_run_again oauth::tests::i5_migration_never_overwrites_an_existing_record oauth::tests::i5_disconnect_during_an_incomplete_upgrade_stays_disconnected server::tests::i6_provider_error_text_is_never_shown server::tests::i7_oauth_status_lists_accounts_without_secrets oauth::tests::i8_two_accounts_sync_at_once_with_their_own_tokens memory::tests::i9_a_fact_seen_in_two_accounts_stays_in_each oauth::tests::account_ids_are_stable_hashes_of_the_subject oauth::tests::admission_table_rows oauth::tests::begin_sends_the_prompt_and_hint_for_each_request oauth::tests::adding_a_listed_account_renews_it_and_keeps_its_data oauth::tests::sign_in_again_as_a_different_account_is_refused oauth::tests::the_cap_refuses_a_new_account_but_not_a_renewal oauth::tests::an_expired_grant_signs_the_account_out oauth::tests::a_stale_invalid_grant_never_signs_out_a_renewed_account oauth::tests::a_failed_record_write_keeps_the_old_entries oauth::tests::migration_files_a_fallback_file_token_as_signed_out oauth::tests::migration_files_a_connection_without_a_subject_under_a_fixed_id oauth::tests::migration_stops_on_an_unreadable_old_subject oauth::tests::failure_codes_map_to_fixed_messages config::tests::a_checked_read_of_a_missing_entry_is_none sources::tests::learn_one_refuses_a_bare_oauth_base store::tests::rename_sources_moves_rows_and_drops_duplicates store::tests::family_queries_match_bare_and_suffixed_names_only store::tests::the_first_migration_backup_survives_a_retry server::tests::provider_level_delete_refuses_when_two_accounts_exist server::tests::provider_level_sync_and_delete_with_zero_and_one_account server::tests::sign_in_records_report_added_renewed_and_failed server::tests::start_picks_the_client_id_in_order server::tests::start_with_an_unknown_account_id_is_404 server::tests::the_sign_in_map_refuses_a_seventeenth_record server::tests::disconnect_fails_waiting_sign_ins_with_a_message server::tests::oauth_status_reports_upgrade_pending_and_client_configured server::tests::bare_rows_can_be_erased_once_the_migration_is_done server::tests::disconnect_source_refuses_legacy_email_while_a_signed_out_account_is_listed ; do printf '%s\n' "$L" \| grep -qx "$t: test" \|\| exit 1; ! printf '%s\n' "$I" \| grep -qx "$t: test" \|\| exit 1; done && printf '%s\n' "$L" \| grep -qx 'oauth::tests::reconnecting_the_same_account_keeps_its_data_vault_live: test'` | hardest tasks: opus high |
| U2 | Mac app: models, wire, client, preview client, store, `ProviderSection`, upgrade notice, copy, tests, re-recorded snapshots. Spec: §5, §5a, §8 upgrade notice, §9, §10 | `macos/HeronMac/**` | none | `swift test --package-path macos/HeronMac && L="$(swift test list --package-path macos/HeronMac)" && for t in testAddsASecondGoogleAccount testAddingAListedAccountShowsRenewed testDisconnectRemovesOnlyThatAccount testFailedSignInShowsTheCoreMessage testFailedFirstSyncShowsTheMessage testCapDisablesAddAnotherAccount testSignInAgainSendsTheAccountID testCancelStopsWaitingLocally testUpgradePendingShowsTheNotice testOAuthProvidersDecodeTheContractLiterals testConnectionsTabWithTwoGoogleAccountsLight ; do printf '%s\n' "$L" \| grep -q "$t" \|\| exit 1; done` | feature, refactoring: sonnet high |
| U3 | Owner sign-in guide. Spec: §11, §12 | `packaging/macos/**` | none | `grep -q 'auth/audience' packaging/macos/README-google-signin.md && grep -q 'AzureADandPersonalMicrosoftAccount' packaging/macos/README-google-signin.md && grep -q 'backups' packaging/macos/README-google-signin.md && grep -q 'before-multi-account' packaging/macos/README-google-signin.md && ! grep -nE 'GOCSPX-\|client_secret=' packaging/macos/README-google-signin.md` | judgment and prose: opus high |
| verify | Full suite and test-quality gate on the merged branch | `**` | U1, U2, U3 | `cargo test --locked && cargo build --locked --features desktop --bins && cargo test --locked --features desktop --bin heron-desktop && cargo clippy --locked --all-targets -- -D warnings && swift test --package-path macos/HeronMac && cargo test --locked --lib -- --ignored --exact oauth::tests::reconnecting_the_same_account_keeps_its_data_vault_live config::tests::connector_vault_live_round_trip && testteeth run --lang rust --base multi-account-base --fail-under 70 -q src/oauth.rs src/sources.rs src/memory.rs src/server.rs src/store.rs src/config.rs` | feature, refactoring: sonnet high |

*   **Escaped pipes.** In the table, `\|` is a pipe inside a check. Each unit's `--completion` gets the plain `|`.
*   **Why the grep check counts to one.** U1 requires exactly one `OUTSIDE_SOURCES.contains` in all of `src/`, inside `src/memory.rs`'s `is_outside`, and no `pub const OUTSIDE_SOURCES`.
*   **Before `skein init`.** Tag the base commit as `multi-account-base`. The `verify` unit's testteeth run uses it.
*   **Where `verify` runs.** On the owner's Mac with the Keychain unlocked, because it runs the two live-vault tests that CI's vault step also runs (`.github/workflows/ci.yml:34`).
*   **Constraints for every work unit:**
    *   the Non-Goals above;
    *   no new crates (`sha2`, `base64`, `serde_json` and `regex` are already in `Cargo.toml`);
    *   no secrets in code, tests or fixtures (tests use made-up tokens such as `refresh-a`);
    *   no required test limited to one OS or marked `#[ignore]`;
    *   Conventional Commits;
    *   units don't commit, because skein commits when a check passes.

--------------------------------------------------------------------------------

*Drafted with EGM*
