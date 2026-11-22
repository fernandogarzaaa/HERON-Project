#!/usr/bin/env bash
# Find the config files a CLI's login creates (M19). Run by the owner, because the login needs a
# browser. Used by U2c.
#
#   bash eval/containment/discover-dirs.sh <id>
#
# Runs the registry entry's `login_command` in the foreground under a fresh temporary HOME, lists
# every file and folder the login created there, and writes them, as `~/...` paths, to
# eval/containment/<id>.dirs.json. Exit 2 for an unknown id, 1 when the login exits non-zero.
# DISCOVER_DIRS_OUT_DIR writes the file somewhere else (the tests use it).
#
# Keychain logins. On macOS some CLIs (claude-cli) save the credential in the login Keychain, and a
# temporary HOME has no Library/Keychains, so the final save fails. That failure is expected here:
# the credential never lives in the home folder, and ~/Library/Keychains is Refused anyway. When the
# login fails with a Keychain error, the script still records what the CLI wrote under the
# temporary HOME, adds the registry's known paths for that CLI (design doc item 22), and marks the
# result `credential_store: "macos-keychain"`, `login_completed: false`. It never links or copies
# the real Keychain, which would overwrite the owner's live login.
#
# The registry is `providers()` in src/cli_llm.rs. A few `login_command` strings are prose, such as
# "agy (then sign in with Google)" or "cline auth <provider>". The script drops a trailing
# parenthetical and runs the rest; if a `<placeholder>` is left, it exits 2 and asks for
# LOGIN_COMMAND, which overrides the registry text: LOGIN_COMMAND='cline auth openai-codex' bash ...
set -eo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

if [ "$#" -ne 1 ]; then
  echo "usage: discover-dirs.sh <id>" >&2
  exit 2
fi
id="$1"
if ! [[ "$id" =~ ^[a-z0-9-]+$ ]]; then
  echo "discover-dirs: unknown id '$id'" >&2
  exit 2
fi

registry="$repo/src/cli_llm.rs"
registry_command="$(python3 -I - "$registry" "$id" <<'PY'
import re
import sys

source = open(sys.argv[1], encoding="utf-8").read()
match = re.search(r'CliSpec \{\s*id: "%s",.*?login_command: "((?:[^"\\]|\\.)*)"' % re.escape(sys.argv[2]), source, re.DOTALL)
if not match:
    sys.exit(2)
print(match.group(1))
PY
)" || {
  echo "discover-dirs: unknown id '$id' (not in $registry)" >&2
  exit 2
}

login="${LOGIN_COMMAND:-$(printf '%s' "$registry_command" | sed -E 's/ \([^)]*\)$//')}"
if [[ "$login" == *"<"*">"* ]]; then
  echo "discover-dirs: the login command '$login' has a placeholder; set LOGIN_COMMAND to the real command" >&2
  exit 2
fi

fake_home="$(mktemp -d "${TMPDIR:-/tmp}/heron-discover.XXXXXX")"
echo "discover-dirs: running '$login' with HOME=$fake_home" >&2
login_log="$(mktemp "${TMPDIR:-/tmp}/heron-discover-log.XXXXXX")"
trap 'rm -rf "$fake_home" "$login_log"' EXIT
status=0
# `script` keeps the login on a real terminal (a browser login can need one) while it records the
# output; on macOS it exits with the login's own status.
if [ "$(uname)" = Darwin ]; then
  script -q "$login_log" env HOME="$fake_home" bash -c "$login" || status=$?
else
  script -q -e -c "env HOME=\"$fake_home\" bash -c \"\$0\"" "$login_log" "$login" || status=$?
fi
store="home-folder"
completed=true
if [ "$status" -ne 0 ]; then
  if grep -qi 'keychain' "$login_log"; then
    echo "discover-dirs: the login could not save to the Keychain under the temporary HOME; that is expected, recording the files it wrote" >&2
    store="macos-keychain"
    completed=false
  else
    echo "discover-dirs: the login exited with status $status" >&2
    exit 1
  fi
fi

case "$id" in
  claude-cli) known='["~/.claude", "~/.claude.json"]' ;;
  *) known='[]' ;;
esac

out="${DISCOVER_DIRS_OUT_DIR:-$here}/$id.dirs.json"
(cd "$fake_home" && find . -mindepth 1 | LC_ALL=C sort) | python3 -I -c '
import json
import sys

id_, login, out, store, completed, known = sys.argv[1:7]
paths = ["~/" + line.rstrip("\n")[2:] for line in sys.stdin if line.strip()]
record = {"id": id_, "login_command": login, "paths": paths,
          "credential_store": store, "login_completed": completed == "true",
          "known_paths": json.loads(known)}
with open(out, "w", encoding="utf-8") as handle:
    json.dump(record, handle, indent=2)
    handle.write("\n")
print("discover-dirs: wrote %d path(s) to %s" % (len(paths), out))
' "$id" "$login" "$out" "$store" "$completed" "$known"
