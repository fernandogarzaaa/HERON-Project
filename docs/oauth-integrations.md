# Google and Microsoft 365 connections (OAuth beta)

**Status: backend implementation under review, not a qualified or complete desktop integration.**
The current native Slint onboarding still contains legacy app-password/ICS copy. Native
Connect/Disconnect controls, user-visible sync errors, live provider tests, and
Google/Microsoft app registration are outstanding; do not present the connector as fully shipped.

## Architecture

The local API implements OAuth 2.0 authorization-code flow with PKCE (S256) for a **public desktop
client**. System-browser consent returns to a **loopback callback**. The callback accepts only
a one-time, ten-minute state created by the authenticated local API. The OAuth code, verifier,
access token and refresh token are never included in model tool schemas, UI JSON, or query-string
launch URLs. No client secret is used. A refresh token is kept through HERON's credential vault
(macOS Keychain / Windows Credential Manager or private local fallback on Linux). Disconnect
clears it and deletes that provider's locally indexed records and associated source memories.

Google permissions: Gmail read-only and Calendar read-only (plus email identity).
Microsoft delegated permissions: Mail.Read, Calendars.Read, User.Read and offline_access.
These connectors do not send mail, alter events or modify a mailbox.

Google records use the sources \`google_email\` and \`google_calendar\`, and Microsoft records
use \`microsoft_email\` and \`microsoft_calendar\`. Legacy IMAP and ICS indexes remain separate.
The source search API combines the provider-specific indexes when the model asks to look at
connected email or calendar data. Each sync imports up to 25 recent inbox messages and up to
50 calendar entries; recurring Google events are expanded by the provider API. Microsoft uses
the next 30 days of calendar entries. Sync repeats with HERON's existing background source
refresh loop, at most every 30 minutes.

**Known limits:** no paging beyond those limits; no calendar-write or email-send capabilities
by design; no offline reconsent handling beyond retry; source learning UI is still tied to the
legacy source names; not tested with live Google or Microsoft accounts; no multi-account
support per provider. The current API must be exercised by an authenticated local client.
Do not distribute production credentials in this repo.

## Provider registration (required before a live test)

Google: register an **OAuth desktop application** in Google Cloud, configure the consent
screen/scopes, enable the Gmail and Google Calendar APIs, and use its *client ID* (not a client
secret). Google may require independent verification for distributing an app that requests
Gmail read-only scopes to general users.

Microsoft: register a **public desktop** application in Microsoft Entra, add the
Mobile and desktop application platform with the \`http://localhost/oauth/callback/microsoft\` loopback redirect (the OS-assigned port is ignored for matching),
allow public-client flows, and grant delegated \`Mail.Read\`, \`Calendars.Read\`, \`User.Read\`
and \`offline_access\`. Some tenants require an administrator's consent.

Consult:
- https://developers.google.com/identity/protocols/oauth2/native-app
- https://developers.google.com/identity/protocols/oauth2/resources/loopback-migration
- https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow
- https://learn.microsoft.com/en-us/azure/active-directory/develop/scenario-desktop-app-registration

## Local API handshake (developer/QA)

Launch HERON bound to 127.0.0.1, take the printed server port and per-launch API
token (never share either externally). The example below uses placeholders.

\`\`\`bash
HERON_URL='http://127.0.0.1:8080'
HERON_TOKEN='<per-launch-api-token>'
CLIENT_ID='<public-desktop-oauth-client-id>'

curl -fsS -X POST "$HERON_URL/api/oauth/google/start" \
  -H "x-heron-token: $HERON_TOKEN" -H 'content-type: application/json' \
  -d "{\"client_id\":\"$CLIENT_ID\"}"
\`\`\`

The JSON \`authorization_url\` is opened in the user's system browser. Google redirects
to HERON's single-use local callback on consent. Microsoft is identical except
\`/api/oauth/microsoft/start\` and a Microsoft public client ID. Redirect errors are
reported in the browser without rendering the code or token.

\`\`\`bash
curl -fsS "$HERON_URL/api/oauth" -H "x-heron-token: $HERON_TOKEN"
curl -fsS -X POST "$HERON_URL/api/oauth/google/sync" -H "x-heron-token: $HERON_TOKEN"
curl -fsS -X DELETE "$HERON_URL/api/oauth/google" -H "x-heron-token: $HERON_TOKEN"
\`\`\`

The synchronous sync endpoint returns counts. The callback initiates a background
sync; a successful OAuth redirect does **not** prove the upstream API read worked.

## Release qualification

1. Run \`cargo test --locked\`, the full native desktop build, cross-platform CI and
   \`oauth::tests\` on the exact PR head.
2. Confirm a Google test user can consent, return to the callback, restart HERON,
   refresh the token, list inbox and calendar, search locally and disconnect.
3. Repeat with a personal Microsoft account and an Entra work account (subject to
   tenant policy). Test consent denial, expired/duplicate state, wrong provider,
   revoked refresh token, network timeout, tenant admin-consent rejection and stale
   sync racing with disconnect.
4. Add and test native Slint connect/disconnect/retry/status controls, credential
   setup, user-visible consent disclosure, empty/loading/error states.
5. Review model prompt-injection safeguards for untrusted mail/calendar entries,
   audit logs for redaction, and the local API trust boundary.
6. Only after passing live acceptance evidence should this be listed as a completed
   OAuth connector in the README or product release notes.
