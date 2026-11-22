#!/usr/bin/env bash
# The self-heal gate: R2 (the new tests catch the bug) and mutation, for a repair PR (item 21).
#
#   bash verify-selfheal.sh <base-sha> <fix-sha> <fix-dir>
#
# <fix-dir> is a checkout of <fix-sha>. The script adds a checkout of <base-sha> next to it
# (`<fix-dir>/../base`), copies the new test files onto it, and writes <fix-dir>/gate-report.json.
# Exit 0: the gate passes (a no_compile_on_base verdict passes, and the report records it).
# Exit 1: a gate failure. Exit 2: a usage error. Set CARGO, SWIFT and TESTTEETH to use other binaries.
set -euo pipefail

usage() {
  echo "verify-selfheal: $1" >&2
  echo "usage: verify-selfheal.sh <base-sha> <fix-sha> <fix-dir>" >&2
  exit 2
}

cargo_bin="${CARGO:-cargo}"
swift_bin="${SWIFT:-swift}"
testteeth_bin="${TESTTEETH:-testteeth}"
swift_package="macos/HeronMac"

# The fix is untrusted input: no hooks, no external diff drivers, no filter processes.
git_safe() {
  git -c core.hooksPath=/dev/null -c core.fsmonitor=false -c diff.external= -c filter.lfs.process= \
    -c core.quotepath=false "$@"
}

[ "$#" -eq 3 ] || usage "expected 3 arguments, got $#"
base="$1"
fix="$2"
for sha in "$base" "$fix"; do
  case "$sha" in
    *[!0-9a-fA-F]*) usage "'$sha' is not a 40-digit commit sha" ;;
  esac
  [ "${#sha}" -eq 40 ] || usage "'$sha' is not a 40-digit commit sha"
done
base="$(printf '%s' "$base" | tr 'A-F' 'a-f')"
fix="$(printf '%s' "$fix" | tr 'A-F' 'a-f')"
[ -d "$3" ] || usage "'$3' is not a directory"
fix_dir="$(cd "$3" && pwd -P)"
for sha in "$base" "$fix"; do
  git_safe -C "$fix_dir" cat-file -e "$sha^{commit}" 2>/dev/null || usage "$sha is not a commit in $fix_dir"
done

base_dir="$(dirname "$fix_dir")/base"
git_safe -C "$fix_dir" worktree add ../base "$base" >&2

changed="$(git_safe -C "$fix_dir" diff --name-only "$base" "$fix")"
added="$(git_safe -C "$fix_dir" diff --diff-filter=A --name-only "$base" "$fix" -- tests macos/HeronMac/Tests connectors)"

rust_stems=""
swift_classes=""
copy_files=""
new_manifest=0
while IFS= read -r path; do
  [ -n "$path" ] || continue
  case "$path" in
    tests/*/*) copy_files="$copy_files$path"$'\n' ;; # a helper module, not a test target
    tests/*.rs)
      stem="${path#tests/}"
      rust_stems="$rust_stems${stem%.rs}"$'\n'
      copy_files="$copy_files$path"$'\n'
      ;;
    tests/*) copy_files="$copy_files$path"$'\n' ;;
    macos/HeronMac/Tests/*.swift)
      class="${path##*/}"
      swift_classes="$swift_classes${class%.swift}"$'\n'
      copy_files="$copy_files$path"$'\n'
      ;;
    macos/HeronMac/Tests/*) copy_files="$copy_files$path"$'\n' ;;
    connectors/*/*) ;;
    connectors/*.toml) new_manifest=1 ;;
  esac
done <<<"$added"

fix_failed=0
base_failed=0
base_no_compile=0
base_passed=0

in_fix() { (cd "$fix_dir" && "$@"); }
in_base() { (cd "$base_dir" && "$@"); }

while IFS= read -r stem; do
  [ -n "$stem" ] || continue
  in_fix "$cargo_bin" test --locked --test "$stem" || fix_failed=1
done <<<"$rust_stems"
while IFS= read -r class; do
  [ -n "$class" ] || continue
  in_fix "$swift_bin" test --package-path "$swift_package" --filter "$class" || fix_failed=1
done <<<"$swift_classes"
if [ "$new_manifest" -eq 1 ]; then
  in_fix "$cargo_bin" test --locked --test connector_manifests || fix_failed=1
fi

have_tests=0
[ -n "$rust_stems$swift_classes" ] && have_tests=1

if [ "$fix_failed" -eq 1 ]; then
  r2="fails_on_fix"
elif [ "$have_tests" -eq 0 ]; then
  if [ "$new_manifest" -eq 1 ]; then r2="new_connector"; else r2="catches_nothing"; fi
else
  # Blobs from the fix commit, so the base run does not depend on what <fix-dir> has checked out.
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    mkdir -p "$base_dir/${path%/*}"
    git_safe -C "$fix_dir" cat-file blob "$fix:$path" >"$base_dir/$path"
  done <<<"$copy_files"

  while IFS= read -r stem; do
    [ -n "$stem" ] || continue
    if ! in_base "$cargo_bin" test --locked --no-run --test "$stem"; then
      base_no_compile=$((base_no_compile + 1))
    elif in_base "$cargo_bin" test --locked --test "$stem"; then
      base_passed=$((base_passed + 1))
    else
      base_failed=$((base_failed + 1))
    fi
  done <<<"$rust_stems"

  if [ -n "$swift_classes" ]; then
    swift_built=1
    in_base "$swift_bin" build --build-tests --package-path "$swift_package" || swift_built=0
    while IFS= read -r class; do
      [ -n "$class" ] || continue
      if [ "$swift_built" -eq 0 ]; then
        base_no_compile=$((base_no_compile + 1))
      elif in_base "$swift_bin" test --package-path "$swift_package" --filter "$class"; then
        base_passed=$((base_passed + 1))
      else
        base_failed=$((base_failed + 1))
      fi
    done <<<"$swift_classes"
  fi

  # One test that fails on base is enough: the fix may also add guards that already held.
  if [ "$base_failed" -gt 0 ]; then
    r2="passes"
  elif [ "$base_no_compile" -gt 0 ]; then
    r2="no_compile_on_base"
  else
    r2="catches_nothing"
  fi
fi

mutation_score="null"
mutation_passed="true"
if printf '%s\n' "$changed" | grep -q '\.rs$'; then
  mutation_status=0
  mutation_out="$(in_fix "$testteeth_bin" run --base "$base" --fail-under 70 -q 2>&1)" || mutation_status=$?
  printf '%s\n' "$mutation_out"
  score="$(printf '%s\n' "$mutation_out" | sed -n 's/.*mutation score: *\([0-9][0-9]*\(\.[0-9][0-9]*\)\{0,1\}\) *%.*/\1/p' | tail -1)"
  [ -z "$score" ] || mutation_score="$score"
  [ "$mutation_status" -eq 0 ] || mutation_passed="false"
fi

json_escape() { printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'; }

changed_json=""
while IFS= read -r path; do
  [ -n "$path" ] || continue
  changed_json="$changed_json${changed_json:+, }\"$(json_escape "$path")\""
done <<<"$changed"

printf '{"base": "%s", "fix": "%s", "r2": "%s", "mutation": {"score": %s, "passed": %s},\n "changed": [%s]}\n' \
  "$base" "$fix" "$r2" "$mutation_score" "$mutation_passed" "$changed_json" >"$fix_dir/gate-report.json"
echo "verify-selfheal: r2=$r2 mutation.passed=$mutation_passed (report: $fix_dir/gate-report.json)" >&2

case "$r2" in
  catches_nothing | fails_on_fix) exit 1 ;;
esac
[ "$mutation_passed" = "true" ] || exit 1
exit 0
