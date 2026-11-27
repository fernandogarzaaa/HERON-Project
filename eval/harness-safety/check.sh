#!/usr/bin/env bash
# bash eval/harness-safety/check.sh <list>...   e.g. check.sh w6a w6a-toolchain
# Every name in eval/harness-safety/tests/<list>.txt must exist, then the whole lib suite runs
# with Seatbelt required, so a Seatbelt test can't pass by skipping.
set -euo pipefail
dir="$(dirname "$0")"
names=()
for list in "$@"; do
  while IFS= read -r name; do [ -n "$name" ] && names+=("$name"); done < "$dir/tests/$list.txt"
done
bash eval/require-tests.sh --lib -- "${names[@]}"
HERON_REQUIRE_SEATBELT=1 cargo test --locked --lib
