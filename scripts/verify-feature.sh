#!/usr/bin/env bash
# The gate for one Heron v1 feature (docs/designs/heron-v1-features.md, base unit B3, item 10).
#
#   bash scripts/verify-feature.sh <slug>
#
# <slug> is `base` for the shared base units (B1 to B5), or a feature's slug (quick_entry, shortcuts,
# ...). Seven steps, in order. The first failing step stops the run. Nothing here loosens a threshold:
# each number comes from the design doc or from the tool that owns it (testteeth, check_gate.py).
#
# BASE, the commit the mutation grade is measured against, comes from eval/features/base.json:
#   slug `base`      -> v1_main_commit
#   any other slug   -> features_base_commit
#
# Step 7 runs only when eval/features/<slug>.md exists. Its output goes outside the repository, so skein
# doesn't commit it, and the short commit id in the path stops --resume from carrying results across
# commits. This script never prints, logs or passes keys or tokens, and it prints no environment.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

STEP="setup"
on_exit() {
  local rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "verify-feature: FAILED at $STEP (exit $rc)" >&2
  fi
}
trap on_exit EXIT

fail() {
  echo "verify-feature: $*" >&2
  exit 1
}

begin() {
  STEP="step $1: $2"
  echo
  echo "==> $STEP"
}

[ "$#" -eq 1 ] || fail "usage: bash scripts/verify-feature.sh <slug>"
SLUG="$1"
case "$SLUG" in
  "" | *[!a-z0-9_]*) fail "the slug must be lowercase letters, digits and underscores: '$SLUG'" ;;
esac

# A Seatbelt test that skips would let a gate run inside a sandboxed shell pass green. With this set,
# every such helper panics instead of skipping (spec: "Skips fail in the gate").
if [ "$(uname -s)" = "Darwin" ]; then
  export HERON_REQUIRE_SEATBELT=1
fi

BASE_JSON="eval/features/base.json"
if [ "$SLUG" = "base" ]; then
  BASE_KEY="v1_main_commit"
else
  BASE_KEY="features_base_commit"
fi
[ -f "$BASE_JSON" ] || fail "$BASE_JSON is missing"
BASE="$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$BASE_JSON" "$BASE_KEY")" \
  || fail "cannot read $BASE_KEY from $BASE_JSON"
case "$BASE" in
  "" | *[!0-9a-f]*) fail "$BASE_KEY in $BASE_JSON is not a commit id: '$BASE'" ;;
esac
[ "${#BASE}" -eq 40 ] || fail "$BASE_KEY in $BASE_JSON is not 40 hex digits: '$BASE'"
git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null \
  || fail "base commit $BASE is not in this repository (fetch the full history)"
echo "verify-feature: slug=$SLUG BASE=$BASE ($BASE_KEY)"

# ----------------------------------------------------------------------------- 1. tests
begin 1 "cargo test"
cargo test --locked
cargo test --locked --features desktop --bin heron-desktop

# ----------------------------------------------------------------------------- 2. clippy
begin 2 "cargo clippy"
cargo clippy --locked --all-targets -- -D warnings

# ----------------------------------------------------------------------------- 3. swift
begin 3 "swift test"
swift test --package-path macos/HeronMac

# ----------------------------------------------------------------------------- 4. unsigned app build
begin 4 "unsigned app build"
bash packaging/macos/build-app.sh --unsigned

# ----------------------------------------------------------------------------- 5. mutation grade
# Skipped when no Rust or Python changed since BASE: task files and base.json don't count. Whether
# testteeth fails when there is nothing to mutate is unverified, so the skip avoids the question.
# `git diff --quiet` exits 0 for no change, 1 for a change and anything else for an error.
begin 5 "testteeth run --base \$BASE --fail-under 70"
DIFF_RC=0
git diff --quiet "$BASE" -- src ':(glob)eval/**/*.py' || DIFF_RC=$?
case "$DIFF_RC" in
  0) echo "testteeth skipped: no Rust or Python change since $BASE" ;;
  1) testteeth run --base "$BASE" --fail-under 70 -q ;;
  *) fail "git diff against $BASE failed (exit $DIFF_RC)" ;;
esac

# ----------------------------------------------------------------------------- 6. genesis audit
begin 6 "genesis audit of the evaluator"
genesis audit-evaluator eval/genesis.yaml --suite code-py

# ----------------------------------------------------------------------------- 7. feature task
TASK_FILE="eval/features/$SLUG.md"
begin 7 "feature task gate ($TASK_FILE)"
if [ -f "$TASK_FILE" ]; then
  OUT="${TMPDIR:-/tmp}/heron-feature-$SLUG-$(git rev-parse --short HEAD)"
  echo "feature tasks: output: $OUT"
  bash eval/run-core-tasks.sh --task-file "$TASK_FILE" --provider claude-cli --driver eve --build debug \
    --runs 3 --out "$OUT" --resume </dev/null
  python3 -I eval/check_gate.py --features "$OUT/summary.json"
else
  echo "no $TASK_FILE: this slug adds no feature task, so there is nothing to run"
fi

STEP="done"
echo
echo "verify-feature: all 7 steps passed for $SLUG"
