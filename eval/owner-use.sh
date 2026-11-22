#!/usr/bin/env bash
# Count the days the owner used Heron (G9).
#
#   bash eval/owner-use.sh <heron-home> <from> <to> [--min <k>]
#
# Dates are YYYY-MM-DD. Counts distinct local days between <from> and <to> with an executed `audit`
# row in a session whose origin is `user` or `telegram`. Prints the days and `qualifying_days=<n>`.
# Exit 0 normally; 1 when --min <k> is given and n < k; 2 on a usage error, a bad date, a window
# that is not exactly 14 days when --min is given (<to> must be 13 days after <from>), a missing
# heron.db, or a data format older than 3 ("needs data format 3").
set -eo pipefail

die() {
  echo "owner-use: $*" >&2
  exit 2
}

if [ "$#" -lt 3 ]; then
  die "usage: owner-use.sh <heron-home> <from> <to> [--min <k>]"
fi
home="$1"
from="$2"
to="$3"
shift 3
min=""
if [ "$#" -gt 0 ]; then
  [ "$1" = "--min" ] && [ "$#" -eq 2 ] || die "usage: owner-use.sh <heron-home> <from> <to> [--min <k>]"
  min="$2"
  [[ "$min" =~ ^[0-9]+$ ]] || die "--min needs a whole number, got '$min'"
fi

date_re='^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
[[ "$from" =~ $date_re ]] || die "<from> must be YYYY-MM-DD, got '$from'"
[[ "$to" =~ $date_re ]] || die "<to> must be YYYY-MM-DD, got '$to'"

command -v sqlite3 >/dev/null 2>&1 || die "sqlite3 is not on PATH"

# The pattern admits 2026-13-45; sqlite's date() only round-trips real calendar days.
for d in "$from" "$to"; do
  [ "$(sqlite3 :memory: "SELECT date('$d')")" = "$d" ] || die "'$d' is not a real date"
done
span="$(sqlite3 :memory: "SELECT CAST(julianday('$to') - julianday('$from') AS INTEGER)")"
[ "$span" -ge 0 ] || die "<to> is before <from>"
if [ -n "$min" ] && [ "$span" -ne 13 ]; then
  die "with --min the window must be 14 consecutive days: <to> must be 13 days after <from>, it is $span"
fi

db="$home/heron.db"
[ -f "$db" ] || die "no heron.db in '$home'"
# Not -readonly: heron.db uses WAL, and a read-only open fails with "unable to open database
# file" when Heron is closed and no heron.db-shm exists. query_only refuses every write instead.
q() { sqlite3 -cmd "PRAGMA query_only=ON;" "$db" "$1"; }
version="$(q "PRAGMA user_version;")"
if [ "$version" -lt 3 ]; then
  die "needs data format 3 (this database is format $version)"
fi

days="$(q "
SELECT DISTINCT date(a.ts, 'unixepoch', 'localtime') AS day
FROM audit a JOIN sessions s ON s.id = a.session_id
WHERE a.decision IN ('allowed', 'approved', 'approved-always', 'approved-for-conversation', 'approved-later')
  AND s.origin IN ('user', 'telegram')
  AND date(a.ts, 'unixepoch', 'localtime') BETWEEN '$from' AND '$to'
ORDER BY day;")"

if [ -n "$days" ]; then
  printf '%s\n' "$days"
  n="$(printf '%s\n' "$days" | grep -c .)"
else
  n=0
fi
echo "qualifying_days=$n"
if [ -n "$min" ] && [ "$n" -lt "$min" ]; then
  exit 1
fi
exit 0
