#!/usr/bin/env bash
# Local-only build of dist/Heron.app with Google (and optionally Microsoft) sign-in compiled in, for installing on this Mac.
#
#   ./packaging/macos/build-local-with-oauth.sh
#
# The Google Desktop OAuth client ID is public. Its client secret is not treated as confidential by Google
# for installed apps (https://developers.google.com/identity/protocols/oauth2/native-app), but it still stays
# out of the repository, CI and logs: it is read here, at build time, from the environment or the macOS
# Keychain (generic password, service "heron-google-oauth-client-secret", account "heron"), exported only
# inside a subshell, and compiled into the core binary. That makes the resulting app yours alone:
# do not distribute it, and do not run this in CI.
#
# Microsoft sign-in is a public client (Entra app: Mobile and desktop platform, redirect
# http://localhost/oauth/callback/microsoft, no secret). Set HERON_MICROSOFT_OAUTH_CLIENT_ID in the
# environment to compile its application (client) ID in; with it unset the Microsoft button stays disabled.
#
# Nothing here echoes the environment.
set -euo pipefail
cd "$(dirname "$0")/../.."

(
  export HERON_GOOGLE_OAUTH_CLIENT_ID="${HERON_GOOGLE_OAUTH_CLIENT_ID:-781568400878-iqlns1buart5fht4464a9o483aq0h89v.apps.googleusercontent.com}"
  if [ -n "${HERON_MICROSOFT_OAUTH_CLIENT_ID:-}" ]; then export HERON_MICROSOFT_OAUTH_CLIENT_ID; fi
  if [ -z "${HERON_GOOGLE_OAUTH_CLIENT_SECRET:-}" ]; then
    secret="$(security find-generic-password -s heron-google-oauth-client-secret -a heron -w 2>/dev/null)" || {
      echo "No HERON_GOOGLE_OAUTH_CLIENT_SECRET in the environment and no Keychain item" >&2
      echo "'heron-google-oauth-client-secret' (account 'heron'). Add it, then run this again." >&2
      exit 1
    }
    export HERON_GOOGLE_OAUTH_CLIENT_SECRET="$secret"
    unset secret
  fi
  exec bash packaging/macos/build-app.sh --unsigned
)
