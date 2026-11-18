#!/usr/bin/env bash
# Fail unless every named test exists in a cargo test selection (M24).
#
#   bash eval/require-tests.sh <cargo test selection> -- <name>...
#
# Runs `cargo test --locked <selection> -- --list` and exits 1, naming the test, unless each
# <name> equals the last `::` component of a listed test exactly, so `a_test` does not match
# `a_test_in_the_sandbox`. Exit 2 is a usage error. Set CARGO to use another cargo binary.
set -eo pipefail

selection=()
while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
  selection+=("$1")
  shift
done
if [ "$#" -eq 0 ]; then
  echo "usage: require-tests.sh <cargo test selection> -- <name>..." >&2
  exit 2
fi
shift
if [ "$#" -eq 0 ]; then
  echo "require-tests: no test names after --" >&2
  exit 2
fi

if ! listing="$("${CARGO:-cargo}" test --locked ${selection[@]+"${selection[@]}"} -- --list 2>/dev/null)"; then
  echo "require-tests: 'cargo test --locked ${selection[*]-} -- --list' failed" >&2
  exit 1
fi

# One line per test, reduced to the last `::` component.
names="$(printf '%s\n' "$listing" | sed -n 's/: test$//p' | sed 's/.*:://')"

status=0
for wanted in "$@"; do
  if ! printf '%s\n' "$names" | grep -Fxq -- "$wanted"; then
    echo "require-tests: no test named '$wanted' in the selection (${selection[*]-})" >&2
    status=1
  fi
done
exit "$status"
