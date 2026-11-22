#!/usr/bin/env bash
# Makes the upgrade-test fixture: a v0.3.1 Heron home folder and the v0.3.1 binary.
#
#   make-upgrade-fixture.sh [--if-missing | --binary-only]
#
# Output:
#   packaging/macos/fixtures/heron-home-0.3.1.tar.gz   the home folder (committed)
#   $OLD_BIN                                           the 0.3.1 binary, outside the worktree
#                                                      (${HERON_OLD_BIN:-${TMPDIR:-/tmp}/heron-0.3.1-bin/heron})
#
# v0.3.1 has no git tag. The fixture is built from commit 9253959, "Prepare HERON v0.3.1 macOS
# packaging fix release". The commit must be reachable, so CI checks out with fetch-depth: 0.
#
#   (no flag)        build the binary, make the archive.
#   --if-missing     do nothing when the archive and the binary both exist; build only the binary when
#                    only the binary is missing; otherwise do everything.
#   --binary-only    build the binary and never touch the archive.
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO=$PWD

COMMIT=9253959
ARCHIVE="$REPO/packaging/macos/fixtures/heron-home-0.3.1.tar.gz"
TMP_ROOT="${TMPDIR:-/tmp}"
TMP_ROOT="${TMP_ROOT%/}"
OLD_BIN="${HERON_OLD_BIN:-$TMP_ROOT/heron-0.3.1-bin/heron}"

MODE=all
case "${1:-}" in
  "") ;;
  --if-missing) MODE=if-missing ;;
  --binary-only) MODE=binary-only ;;
  *) echo "usage: make-upgrade-fixture.sh [--if-missing | --binary-only]" >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { echo "usage: make-upgrade-fixture.sh [--if-missing | --binary-only]" >&2; exit 2; }

BUILD_BIN=1
MAKE_ARCHIVE=1
if [ "$MODE" = binary-only ]; then
  MAKE_ARCHIVE=0
elif [ "$MODE" = if-missing ]; then
  if [ -f "$ARCHIVE" ] && [ -x "$OLD_BIN" ]; then
    echo "Fixture already there: $ARCHIVE and $OLD_BIN"
    exit 0
  fi
  if [ -x "$OLD_BIN" ]; then BUILD_BIN=0; fi
  if [ -f "$ARCHIVE" ]; then MAKE_ARCHIVE=0; fi
fi

if ! git cat-file -e "$COMMIT^{commit}" 2>/dev/null; then
  if [ "$BUILD_BIN" = 1 ]; then
    echo "commit $COMMIT (the v0.3.1 release prep) is not in this clone. Fetch full history: git fetch --unshallow" >&2
    exit 1
  fi
fi

WORK=$(mktemp -d "$TMP_ROOT/heron-fixture.XXXXXX")
SRC="$WORK/heron-0.3.1"
USED_WORKTREE=0
cleanup() {
  if [ "$USED_WORKTREE" = 1 ]; then
    git -C "$REPO" worktree remove --force "$SRC" >/dev/null 2>&1 || true
  fi
  # A failed `worktree add` can leave a half-made entry behind.
  git -C "$REPO" worktree prune >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# Step 1: the 0.3.1 sources. A git worktree, as in the spec; if git can't make one (a read-only
# .git, for example) the same tree is exported with git archive instead.
fetch_source() {
  if git -C "$REPO" worktree add --detach "$SRC" "$COMMIT" >/dev/null 2>&1; then
    USED_WORKTREE=1
  else
    echo "git worktree add failed; exporting $COMMIT with git archive instead" >&2
    rm -rf "$SRC"
    mkdir -p "$SRC"
    git -C "$REPO" archive "$COMMIT" | tar -x -C "$SRC"
  fi
}

# Step 2: build the binary (B1).
if [ "$BUILD_BIN" = 1 ]; then
  fetch_source
  (cd "$SRC" && cargo build --locked --release --bin heron)
  mkdir -p "$(dirname "$OLD_BIN")"
  cp "$SRC/target/release/heron" "$OLD_BIN"
  chmod +x "$OLD_BIN"
  echo "Built the v0.3.1 binary at $OLD_BIN"
fi

[ "$MAKE_ARCHIVE" = 1 ] || exit 0
[ -x "$OLD_BIN" ] || { echo "$OLD_BIN is missing or not executable" >&2; exit 1; }
command -v sqlite3 >/dev/null || { echo "sqlite3 is needed to make the fixture" >&2; exit 1; }

# Step 3: sessions, messages and audit rows, made by the real 0.3.1 binary in demo mode.
export HERON_HOME="$WORK/home"
FOLDER="$WORK/work"
mkdir -p "$HERON_HOME" "$FOLDER"
chmod 700 "$HERON_HOME"
cat > "$HERON_HOME/config.toml" <<'EOF'
[llm]
base_url = "demo"
EOF
for prompt in "Save plan.md" "Add a task to try Heron" "List the folder"; do
  "$OLD_BIN" -C "$FOLDER" --mode yolo run "$prompt" </dev/null >/dev/null
done

DB="$HERON_HOME/heron.db"
[ -f "$DB" ] || { echo "the v0.3.1 binary made no $DB" >&2; exit 1; }

# Step 4: two memories, one task and one schedule.
sqlite3 "$DB" <<'SQL'
INSERT INTO memories(kind, content, tags, created_at, updated_at)
  VALUES ('fact', 'The fixture owner likes plain-text notes', 'fixture', strftime('%s','now'), strftime('%s','now')),
         ('fact', 'The fixture owner reviews tasks on Mondays', 'fixture', strftime('%s','now'), strftime('%s','now'));
INSERT INTO tasks(title, created_at) VALUES ('Fixture task: read the upgrade notes', strftime('%s','now'));
INSERT INTO schedules(name, cron, prompt, created_at)
  VALUES ('Fixture schedule', '0 9 * * 1', 'List the folder', strftime('%s','now'));
SQL

# Step 5: allow rules, in the config file and in the kv store.
cat >> "$HERON_HOME/config.toml" <<'EOF'

[permissions]
allow = ["shell:git *", "read_file:*"]
EOF
sqlite3 "$DB" "INSERT OR REPLACE INTO kv(k, v) VALUES ('allow_rules', '[\"shell:git *\",\"read_file:*\"]');"

# The fixture must be a 0.3.1 database with data in it. Fail here rather than ship an empty one.
version=$(sqlite3 "$DB" "PRAGMA user_version;")
[ "$version" = 2 ] || { echo "the fixture database is at data format $version, not 2" >&2; exit 1; }
for table in sessions messages memories tasks schedules audit; do
  n=$(sqlite3 "$DB" "SELECT count(*) FROM $table;")
  [ "$n" -gt 0 ] || { echo "the fixture has no rows in $table" >&2; exit 1; }
done

# Step 6: archive the home folder. Fold the write-ahead log into the file first, so the archive
# holds one self-contained database.
sqlite3 "$DB" "PRAGMA wal_checkpoint(TRUNCATE);" >/dev/null
rm -f "$DB-wal" "$DB-shm"
mkdir -p "$(dirname "$ARCHIVE")"
# COPYFILE_DISABLE keeps macOS from adding ._ resource-fork entries to the archive.
COPYFILE_DISABLE=1 tar -C "$HERON_HOME" -czf "$ARCHIVE" .
echo "Wrote $ARCHIVE"
