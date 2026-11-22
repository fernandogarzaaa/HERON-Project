#!/usr/bin/env bash
# Fail unless every test a graph requires exists (the verify node of each graph in the harness2 design).
#
#   bash eval/selfheal/check-required.sh <graph>
#
# Reads eval/selfheal/required-tests.txt (or the file named by REQUIRED_TESTS), whose lines are
# `<graph> <selection> <name>`. It groups the lines of <graph> by selection and runs
# `bash eval/require-tests.sh <selection> -- <names>` once for each group, so CARGO overrides the cargo
# binary as it does there. Every group runs even after one fails. Exit 0 when all pass, 1 when any
# required test is missing, 2 on a usage error or a malformed list.
set -eo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
list="${REQUIRED_TESTS:-$here/required-tests.txt}"

if [ "$#" -ne 1 ]; then
  echo "usage: check-required.sh <graph>" >&2
  exit 2
fi
graph="$1"
if [ ! -f "$list" ]; then
  echo "check-required: no such list: $list" >&2
  exit 2
fi

# Blank lines and # comments are skipped. A selection is one word, so `--test=integration`, not `--test integration`.
bad="$(awk '
  /^[[:space:]]*(#|$)/ { next }
  NF != 3 || $1 !~ /^[0-9]+[a-z]$/ || $2 !~ /^--[a-z-]+(=[A-Za-z0-9_]+)?$/ || $3 !~ /^[A-Za-z_][A-Za-z0-9_]*$/ {
    printf "line %d: %s\n", NR, $0
  }' "$list")"
if [ -n "$bad" ]; then
  echo "check-required: malformed line in $list:" >&2
  echo "$bad" >&2
  exit 2
fi

selections="$(awk -v g="$graph" '$1 == g && !seen[$2]++ { print $2 }' "$list")"
if [ -z "$selections" ]; then
  echo "check-required: no required tests for graph '$graph' in $list" >&2
  exit 2
fi

status=0
for selection in $selections; do
  names="$(awk -v g="$graph" -v s="$selection" '$1 == g && $2 == s && !seen[$3]++ { print $3 }' "$list")"
  # shellcheck disable=SC2086 # the names are single words, checked above
  if ! bash "$here/../require-tests.sh" "$selection" -- $names; then
    status=1
  fi
done
exit "$status"
