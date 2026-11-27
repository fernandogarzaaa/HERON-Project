#!/usr/bin/env bash
# Containment check on every installed candidate CLI (work unit U2c, design item 31).
#
#   bash eval/containment/run.sh
#
# For each candidate, in order (claude-cli, opencode-cli, cline-cli, codex-cli, antigravity-cli,
# gemini-cli), it writes eval/containment/<id>.json:
#   * binary not found                      -> {"id", "status": "not-installed"}
#   * <id>.dirs.json has unavailable_reason -> {"id", "status": "unavailable", "reason"}, nothing run
#   * otherwise it runs the real-CLI canary, `canary_real_cli` in src/cli_llm.rs, then rewrites its
#     report as the result record (see results.py).
# A CLI that fails its containment check is recorded as failed; that is a result, not a script error.
# The script exits 1 only when a canary run produced no report at all (cargo failed to build or run),
# so a broken run can't be mistaken for a measurement. The previous <id>.json is removed first, so a
# stale file never stands in for a run.
#
# Run it on the owner's Mac, outside any sandbox, with each candidate installed and logged in. It
# spends a little of each subscription's quota (one turn per CLI).
#
# Knobs (the tests use them): CARGO (cargo binary), CONTAINMENT_OUT_DIR (where <id>.json and
# <id>.dirs.json live, default this folder), CONTAINMENT_SYSTEM_DIRS (replaces /opt/homebrew/bin and
# /usr/local/bin when looking for binaries).
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
out_dir="${CONTAINMENT_OUT_DIR:-$here}"
export CONTAINMENT_OUT_DIR="$out_dir"
cargo_bin="${CARGO:-cargo}"
results="$here/results.py"

candidates="claude-cli:claude opencode-cli:opencode cline-cli:cline codex-cli:codex antigravity-cli:agy gemini-cli:gemini"
broken=0

for pair in $candidates; do
  id="${pair%%:*}"
  binary="${pair##*:}"
  out="$out_dir/$id.json"
  rm -f "$out"

  if ! found="$(python3 -I "$results" find "$binary")"; then
    echo "containment: $id: '$binary' is not installed" >&2
    python3 -I "$results" simple "$id" not-installed || broken=1
    continue
  fi
  if python3 -I "$results" unavailable "$id"; then
    echo "containment: $id: unavailable (see $id.dirs.json); nothing run" >&2
    continue
  fi

  echo "containment: $id: running the canary against $found" >&2
  # The folder of a binary found outside $PATH goes on the end of $PATH, so Heron's resolver finds
  # the same binary this script did.
  (cd "$repo" && PATH="$PATH:$(dirname "$found")" HERON_CANARY_PROVIDER="$id" HERON_CANARY_OUT="$out" \
    "$cargo_bin" test --locked --lib canary_real_cli -- --ignored)
  status=$?
  if [ ! -s "$out" ]; then
    echo "containment: $id: cargo exited $status and wrote no canary report; not recorded" >&2
    broken=1
    continue
  fi
  if [ "$status" -ne 0 ]; then
    echo "containment: $id: cargo exited $status, but the canary report is there; recording it" >&2
  fi
  if ! python3 -I "$results" result "$id"; then
    rm -f "$out"
    broken=1
  fi
done

if [ "$broken" -ne 0 ]; then
  echo "containment: at least one candidate has no result; fix the cause and run again" >&2
  exit 1
fi
