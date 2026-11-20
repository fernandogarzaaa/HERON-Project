#!/usr/bin/env bash
# The full gate for Heron v1 (docs/designs/heron-v1.md, work unit verify, item 48).
#
#   bash scripts/verify-v1.sh
#
# Eleven steps, in order. The first failing step stops the run, except step 7, which goes through
# every Tier 1 CLI before it fails. Nothing here loosens a threshold: each number comes from the
# design doc or from the tool that owns it (testteeth, check_gate.py, owner-use.sh).
#
# Environment:
#   HERON_TRIAL_FROM, HERON_TRIAL_TO  the 14-day owner trial (YYYY-MM-DD). Both set: step 9 measures G9.
#   HERON_G9_PENDING=1                the build session's way past step 9 while the trial is running.
#                                     The second pull request needs a run without it.
#
# Step 7 uses the owner's existing CLI logins. This script never prints, logs or passes keys or
# tokens, and it prints no environment.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

STEP="setup"
on_exit() {
  local rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "verify-v1: FAILED at $STEP (exit $rc)" >&2
  fi
}
trap on_exit EXIT

fail() {
  echo "verify-v1: $*" >&2
  exit 1
}

begin() {
  STEP="step $1: $2"
  echo
  echo "==> $STEP"
}

# A Seatbelt test that skips would let a gate run inside a sandboxed shell pass green. With this set,
# every such helper panics instead of skipping (spec: "Skips fail in the gate").
if [ "$(uname -s)" = "Darwin" ]; then
  export HERON_REQUIRE_SEATBELT=1
fi

# BASE is the commit U0b measured: the start of feat/heron-v1 (main plus eval/, no src/ change).
BASE="$(python3 -I -c 'import json; print(json.load(open("eval/baseline/summary.json"))["heron_commit"])')" \
  || fail "cannot read heron_commit from eval/baseline/summary.json"
case "$BASE" in
  "" | *[!0-9a-f]*) fail "heron_commit in eval/baseline/summary.json is not a commit id: '$BASE'" ;;
esac
[ "${#BASE}" -eq 40 ] || fail "heron_commit in eval/baseline/summary.json is not 40 hex digits: '$BASE'"
git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null \
  || fail "baseline commit $BASE is not in this repository (fetch the full history)"
echo "verify-v1: BASE=$BASE"

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

# ----------------------------------------------------------------------------- 4. mutation grade
# Rust (cargo-mutants, default features, so the desktop bin isn't mutated) and the Python under eval/.
begin 4 "testteeth run --base \$BASE --fail-under 70"
testteeth run --base "$BASE" --fail-under 70 -q

# ----------------------------------------------------------------------------- 5. G3 record
# The new tests call APIs the old code doesn't have, so copying them onto BASE proves nothing. The G3
# reviewer reproduced each scenario against BASE and wrote one line per test:
#   <test name>: fails on <BASE>: <one-line evidence>
# This step only checks the record.
begin 5 "G3 regressions fail on the old code (docs/designs/reviews/g3-regressions.md)"
python3 -I - "$BASE" docs/designs/reviews/g3-regressions.md <<'PY' || fail "the G3 record is missing or incomplete (see above)"
import re
import sys

base, path = sys.argv[1], sys.argv[2]
NAMES = [
    "m1_injected_file_cannot_read_and_send_a_credential",
    "m2_mcp_cannot_plant_memories_or_schedules",
    "m4_globs_quotes_and_unlisted_secrets_ask",
    "m4_sandbox_denies_globs_quotes_and_unlisted_secrets",
    "m6_proactive_sessions_start_tainted",
    "m7_parked_call_keeps_its_taint",
    "m8_forwarded_telegram_message_taints",
]
try:
    with open(path, "r", encoding="utf-8") as handle:
        lines = [line.strip() for line in handle.read().splitlines()]
except OSError as exc:
    print("G3: cannot read %s: %s" % (path, exc))
    sys.exit(1)

missing = []
for name in NAMES:
    pattern = re.compile(r"^[>*+-]*\s*`?%s`?: fails on `?([0-9a-f]{7,40})`?: (\S.*)$" % re.escape(name))
    found = False
    for line in lines:
        match = pattern.match(line)
        if match and base.startswith(match.group(1)):
            found = True
            break
    if not found:
        missing.append(name)
if missing:
    print("G3: %s has no valid line for:" % path)
    for name in missing:
        print("  - %s" % name)
    print("G3: each line reads `<test name>: fails on <BASE>: <one-line evidence>`, with BASE = %s" % base)
    sys.exit(1)
print("G3: %s has a line for each of the %d m* tests" % (path, len(NAMES)))
PY

# ----------------------------------------------------------------------------- 6. genesis audits
begin 6 "genesis audits of the evaluators"
genesis audit-evaluator eval/genesis.yaml --suite code-py
genesis audit-evaluator eval/genesis/pass-rules-json.yaml --suite json
genesis audit-evaluator eval/genesis/limit-classifier.yaml --suite json
python3 -I -m unittest discover -s eval/tests -t eval -p test_g7.py

# ----------------------------------------------------------------------------- 7. G7
# For every Tier 1 CLI, one at a time, the 10 gate tasks and the R-1 probe, 3 runs each, through the
# existing runner (mode = "ask"). One task per runner call, so a provider limit stops a CLI after the
# task it hit instead of after 33 runs.
#
# The runner rewrites summary.json with only the tasks it was asked for, so each call's summary is
# kept in <out>/parts/<task>.json and the parts are merged back into <out>/summary.json after every
# task. That keeps summary.json whole for check_gate.py and for a later --resume.
#
# The output stays outside the repository, so skein doesn't commit it.
# Step 7's helpers (task ids, merge, limit scan, report) live in eval/g7.py, where
# eval/tests/test_g7.py checks the limit classifier against eval/fixtures/g7_limit_samples.json.

g7py() {
  python3 -I eval/g7.py "$@"
}

# One CLI. Returns 0 when its gate passed, 1 when it failed or stopped. Written with explicit status
# handling because `set -e` doesn't apply inside a function that is called from `||`.
g7_one_cli() {
  local id="$1" out="$2" tasks="$3" tid rc old stopped=0 failed=0

  # --resume keeps runs from an earlier gate run. Keep them only when the code under test and the
  # tasks are unchanged since that run; otherwise old results must not count for the new code.
  if [ -f "$out/summary.json" ]; then
    old="$(g7py commit "$out")" || old=""
    if [ -z "$old" ] || ! git diff --quiet "$old" -- src eval Cargo.toml Cargo.lock rust-toolchain.toml 2>/dev/null; then
      local aside="$out.stale-$(date +%Y%m%d-%H%M%S)"
      echo "G7 $id: earlier results were made on other code (${old:-unknown commit}); moved to $aside"
      mv "$out" "$aside" || return 1
    else
      echo "G7 $id: resuming the results in $out (same code as $old)"
    fi
  fi
  mkdir -p "$out/parts" || return 1

  for tid in $tasks; do
    echo "G7 $id: task $tid"
    rc=0
    bash eval/run-core-tasks.sh --provider "$id" --driver eve --runs 3 --out "$out" --tasks "$tid" --resume \
      </dev/null || rc=$?
    if [ "$rc" -ge 2 ]; then
      echo "G7 $id: the runner could not run task $tid (exit $rc)"
      failed=1
      break
    fi
    if [ ! -f "$out/summary.json" ]; then
      echo "G7 $id: the runner wrote no summary.json for task $tid"
      failed=1
      break
    fi
    cp "$out/summary.json" "$out/parts/$tid.json" || { failed=1; break; }
    g7py merge "$out" $tasks || { echo "G7 $id: cannot merge the summaries"; failed=1; break; }
    rc=0
    g7py scan "$id" "$out" "$tid" || rc=$?
    if [ "$rc" -eq 10 ]; then
      stopped=1 # g7py printed the STOPPED line; no retry loop, go on to the next CLI
      break
    elif [ "$rc" -ne 0 ]; then
      echo "G7 $id: cannot read the results of task $tid"
      failed=1
      break
    fi
  done

  if [ -f "$out/summary.json" ]; then
    g7py report "$id" "$out" || failed=1
  fi
  if [ "$stopped" -eq 1 ]; then
    echo "G7 $id: check_gate skipped, because G7 was not measured in full. A later --resume rerun completes the missing runs."
    return 1
  fi
  if [ "$failed" -eq 1 ]; then
    return 1
  fi
  python3 -I eval/check_gate.py "$out/summary.json" || return 1
  return 0
}

begin 7 "G7 task gate on every Tier 1 CLI"
G7_ROOT="${TMPDIR:-/tmp}/heron-verify-eval"
G7_IDS="$(python3 -I eval/containment/check.py --list-tier1)" || fail "check.py --list-tier1 failed"
[ -n "$G7_IDS" ] || fail "check.py --list-tier1 printed no CLI"
G7_TASKS="$(g7py ids)" || fail "cannot read the task ids from eval/core-tasks.md"
[ -n "$G7_TASKS" ] || fail "eval/core-tasks.md has no tasks"
echo "G7: CLIs: $(echo $G7_IDS); tasks: $(echo $G7_TASKS); output: $G7_ROOT/<id>"
G7_BAD=""
for id in $G7_IDS; do
  if ! g7_one_cli "$id" "$G7_ROOT/$id" "$G7_TASKS"; then
    G7_BAD="$G7_BAD $id"
  fi
done
[ -z "$G7_BAD" ] || fail "G7 failed or was not measured in full for:$G7_BAD"

# ----------------------------------------------------------------------------- 8. G4
begin 8 "containment check (G4)"
python3 -I eval/containment/check.py

# ----------------------------------------------------------------------------- 9. G9
begin 9 "G9 owner use"
if [ -n "${HERON_TRIAL_FROM:-}" ] && [ -n "${HERON_TRIAL_TO:-}" ]; then
  bash eval/owner-use.sh "$HOME/.heron" "$HERON_TRIAL_FROM" "$HERON_TRIAL_TO" --min 12
elif [ "${HERON_G9_PENDING:-}" = "1" ]; then
  echo "G9 not measured yet: HERON_G9_PENDING=1 lets the build session continue. The second pull request needs a run without it."
else
  fail "G9 not measured yet (set HERON_TRIAL_FROM and HERON_TRIAL_TO once the 14-day trial has ended)"
fi

# ----------------------------------------------------------------------------- 10. release build
begin 10 "unsigned app build and upgrade test"
bash packaging/macos/build-app.sh --unsigned
bash packaging/macos/upgrade-test.sh

# ----------------------------------------------------------------------------- 11. register
begin 11 "accepted-risk register"
[ -f docs/ACCEPTED_RISKS.md ] || fail "docs/ACCEPTED_RISKS.md is missing"
if grep -q "Awaiting the owner's signature" docs/ACCEPTED_RISKS.md; then
  fail "docs/ACCEPTED_RISKS.md still has an entry awaiting the owner's signature"
fi
R_COUNT="$(grep -c '^## R-' docs/ACCEPTED_RISKS.md || true)"
[ "$R_COUNT" -le 3 ] || fail "docs/ACCEPTED_RISKS.md has $R_COUNT '## R-' headings; at most 3 are allowed"

STEP="done"
echo
echo "verify-v1: all 11 steps passed"
