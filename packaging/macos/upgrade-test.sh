#!/usr/bin/env bash
# Upgrade test: a v0.3.1 Heron home folder is opened by the binary that ships, and must survive.
#
#   packaging/macos/upgrade-test.sh
#
# Needs, from the repository root:
#   packaging/macos/fixtures/heron-home-0.3.1.tar.gz   (make-upgrade-fixture.sh)
#   $NEW_BIN = ${HERON_NEW_BIN:-dist/Heron.app/Contents/MacOS/heron-cli}   (build-app.sh)
#   $OLD_BIN = ${HERON_OLD_BIN:-${TMPDIR:-/tmp}/heron-0.3.1-bin/heron}     (make-upgrade-fixture.sh)
#
# Checks: the data, the allow rules and the audit chain survive; the pre-upgrade snapshot was made;
# and going back to the snapshot with the old binary works. The keychain is not covered (G8).
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO=$PWD

ARCHIVE="$REPO/packaging/macos/fixtures/heron-home-0.3.1.tar.gz"
NEW_BIN="${HERON_NEW_BIN:-$REPO/dist/Heron.app/Contents/MacOS/heron-cli}"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP_ROOT="${TMP_ROOT%/}"
OLD_BIN="${HERON_OLD_BIN:-$TMP_ROOT/heron-0.3.1-bin/heron}"

if [ ! -x "$NEW_BIN" ]; then
  echo "$NEW_BIN is missing: run packaging/macos/build-app.sh first" >&2
  exit 2
fi
if [ ! -f "$ARCHIVE" ]; then
  echo "$ARCHIVE is missing: run packaging/macos/make-upgrade-fixture.sh first" >&2
  exit 2
fi
command -v sqlite3 >/dev/null || { echo "sqlite3 is needed for the upgrade test" >&2; exit 2; }

WORK=$(mktemp -d "$TMP_ROOT/heron-upgrade.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
fail() { echo "upgrade-test FAILED: $*" >&2; exit 1; }

# 1. Unpack the fixture; HERON_HOME points at it. The folder is private, as Heron requires.
export HERON_HOME="$WORK/home"
mkdir -p "$HERON_HOME"
chmod 700 "$HERON_HOME"
tar -xzf "$ARCHIVE" -C "$HERON_HOME"
DB="$HERON_HOME/heron.db"
[ -f "$DB" ] || fail "the fixture holds no heron.db"

# 2. Record. The queries below are SELECT and PRAGMA reads only, so no data changes.
# Not `-readonly`: the fixture is in WAL mode and the archive holds no heron.db-shm, and a
# read-only open can't create it (SQLITE_CANTOPEN, 14). Not `?immutable=1` either: once the new
# binary has run, data can sit in heron.db-wal and an immutable open would ignore it. A normal
# open may create ordinary -wal/-shm files, which is fine.
q() { sqlite3 "$DB" "$1"; }
TABLES="sessions messages memories tasks schedules audit"
counts() {
  local t
  for t in $TABLES; do echo "$t=$(q "SELECT count(*) FROM $t;")"; done
}
rules_kv() { q "SELECT v FROM kv WHERE k='allow_rules';"; }
rules_cfg() { grep -E '^[[:space:]]*allow[[:space:]]*=' "$HERON_HOME/config.toml" || true; }

BEFORE_COUNTS=$(counts)
BEFORE_KV=$(rules_kv)
BEFORE_CFG=$(rules_cfg)
BEFORE_VERSION=$(q "PRAGMA user_version;")

[ "$BEFORE_VERSION" = 2 ] || fail "the fixture is at data format $BEFORE_VERSION, expected 2"
[ -n "$BEFORE_KV" ] || fail "the fixture has no allow_rules in the kv store"
[ -n "$BEFORE_CFG" ] || fail "the fixture has no allow list in config.toml"
case "$BEFORE_COUNTS" in
  *"=0"$'\n'*|*"=0") fail "the fixture has an empty table:"$'\n'"$BEFORE_COUNTS" ;;
esac
ls "$HERON_HOME/backups"/heron-before-upgrade-from-v2-*.db >/dev/null 2>&1 \
  && fail "the fixture already holds an upgrade snapshot; it would hide a missing one"

# 3. Open the store with the binary that ships. Opening it makes the snapshot and migrates.
AUDIT_OUT="$WORK/audit.out"
if ! "$NEW_BIN" audit >"$AUDIT_OUT" 2>"$WORK/audit.err"; then
  cat "$AUDIT_OUT" "$WORK/audit.err" >&2
  fail "'$NEW_BIN audit' did not exit 0"
fi
grep -q "Activity log intact" "$AUDIT_OUT" || { cat "$AUDIT_OUT" >&2; fail "the audit chain check did not report an intact log"; }
if grep -q "WARNING" "$AUDIT_OUT"; then cat "$AUDIT_OUT" >&2; fail "the audit chain reports a broken entry"; fi

# 4. The format moved forward, nothing was lost, the snapshot exists.
AFTER_VERSION=$(q "PRAGMA user_version;")
[ "$AFTER_VERSION" = 3 ] || fail "PRAGMA user_version is $AFTER_VERSION after the upgrade, expected 3"
AFTER_COUNTS=$(counts)
[ "$AFTER_COUNTS" = "$BEFORE_COUNTS" ] || fail "row counts changed:"$'\n'"before:"$'\n'"$BEFORE_COUNTS"$'\n'"after:"$'\n'"$AFTER_COUNTS"
[ "$(rules_kv)" = "$BEFORE_KV" ] || fail "the allow_rules kv entry changed: $BEFORE_KV -> $(rules_kv)"
[ "$(rules_cfg)" = "$BEFORE_CFG" ] || fail "the config allow list changed: $BEFORE_CFG -> $(rules_cfg)"
SNAPSHOT=
for f in "$HERON_HOME/backups"/heron-before-upgrade-from-v2-*.db; do
  [ -e "$f" ] && SNAPSHOT=$f
done
[ -n "$SNAPSHOT" ] || fail "no backups/heron-before-upgrade-from-v2-*.db was made"

# 5. Rehearse the rollback with the old binary.
if [ ! -x "$OLD_BIN" ]; then
  HERON_OLD_BIN="$OLD_BIN" bash "$REPO/packaging/macos/make-upgrade-fixture.sh" --binary-only
fi
[ -x "$OLD_BIN" ] || fail "$OLD_BIN is still missing after make-upgrade-fixture.sh --binary-only"
"$OLD_BIN" restore "$SNAPSHOT" >"$WORK/restore.out" 2>&1 || { cat "$WORK/restore.out" >&2; fail "'$OLD_BIN restore' did not exit 0"; }
RESTORED_VERSION=$(q "PRAGMA user_version;")
[ "$RESTORED_VERSION" = 2 ] || fail "PRAGMA user_version is $RESTORED_VERSION after the rollback, expected 2"
"$OLD_BIN" audit >"$WORK/old-audit.out" 2>&1 || { cat "$WORK/old-audit.out" >&2; fail "the old binary's audit did not exit 0 after the rollback"; }

echo "upgrade-test OK: v0.3.1 -> current (data format 2 -> 3), counts and rules kept, snapshot made, rollback works"
