#!/usr/bin/env bash
# Measurement spike for claude-cli's topology, envelope vs hybrid (work unit U2s, design item 32, G5).
#
#   bash eval/spike/run-spike.sh [--resume]
#
# Runs the ten gate tasks t01..t10, 5 runs each, on claude-cli in `ask` mode with a debug build, once per
# arm (50 runs per arm), then writes:
#   eval/spike/envelope/summary.json, eval/spike/hybrid/summary.json   the two arms
#   eval/spike/topology.md   `Chosen default: topology=<value>`, a table per arm, the reason
#   eval/spike/pairs.md      one blind pair of final replies per task, in random order, unlabelled
#   eval/spike/pairs-key.json  which arm is A and B in each pair, for the owner's blind rating
# It only measures. U2w applies the result.
#
# Run it on the owner's Mac, outside any sandbox, with claude logged in. It spends subscription quota:
# about 100 model turns. Without --resume it starts both arms from scratch (an existing summary.json is
# kept as summary.previous.json). With --resume it skips runs already recorded without an error, and
# refuses a summary made at another commit, so both arms always measure the same code.
#
# Exit codes: 0 the spike ran and the reports are written; 1 a run still has an error after its retry
# (re-run with --resume), or the arms can't be compared; 2 usage; 3 `claude` isn't installed.
# Per-run artifacts go to a temporary folder, removed on success and kept (and named) on failure.
#
# Knobs (the tests use them): SPIKE_RUNNER (replaces eval/run-core-tasks.sh), SPIKE_OUT_DIR (replaces
# this folder).
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
out_dir="${SPIKE_OUT_DIR:-$here}"
runner="${SPIKE_RUNNER:-$repo/eval/run-core-tasks.sh}"
tasks="t01,t02,t03,t04,t05,t06,t07,t08,t09,t10"

resume=()
case "${1-}" in
  "") ;;
  --resume) resume=(--resume) ;;
  *) echo "usage: bash eval/spike/run-spike.sh [--resume]" >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { echo "usage: bash eval/spike/run-spike.sh [--resume]" >&2; exit 2; }

if ! command -v claude >/dev/null 2>&1; then
  echo "run-spike: 'claude' is not on PATH; install and log in to Claude Code first. Nothing was run." >&2
  exit 3
fi

head_commit="$(git -C "$repo" rev-parse HEAD 2>/dev/null || echo unknown)"

# One summary's heron_commit, or nothing when there is no summary.
summary_commit() {
  [ -f "$1" ] || return 0
  python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1])).get("heron_commit",""))' "$1" 2>/dev/null || true
}

for arm in envelope hybrid; do
  summary="$out_dir/$arm/summary.json"
  if [ ${#resume[@]} -gt 0 ]; then
    prior="$(summary_commit "$summary")"
    if [ -n "$prior" ] && [ "$prior" != "$head_commit" ]; then
      echo "run-spike: $summary was made at commit $prior, not $head_commit; the arms must measure the same" \
           "code. Move that arm's folder away and run without --resume." >&2
      exit 1
    fi
  elif [ -f "$summary" ]; then
    echo "run-spike: $summary exists; starting over and keeping the old one as summary.previous.json" >&2
    mv -f "$summary" "$out_dir/$arm/summary.previous.json"
  fi
done

artifacts="$(mktemp -d "${TMPDIR:-/tmp}/heron-spike.XXXXXX")" || exit 1

for arm in envelope hybrid; do
  mkdir -p "$out_dir/$arm"
  echo "run-spike: $arm arm (5 runs of each of t01..t10)" >&2
  if ! bash "$runner" --provider claude-cli --build debug --runs 5 --tasks "$tasks" --topology "$arm" \
       --out "$out_dir/$arm" --artifacts "$artifacts/$arm" ${resume[@]+"${resume[@]}"}; then
    echo "run-spike: the $arm arm did not finish cleanly; per-run artifacts are in $artifacts." >&2
    echo "run-spike: fix the cause and re-run with --resume to continue from the last finished run." >&2
    exit 1
  fi
done

python3 -I "$repo/eval/check_gate.py" --spike "$out_dir/envelope/summary.json" "$out_dir/hybrid/summary.json" \
  --write-report "$out_dir/topology.md" || { echo "run-spike: the arms can't be compared (see above)" >&2; exit 1; }
python3 -I "$here/make_pairs.py" "$out_dir/envelope/summary.json" "$out_dir/hybrid/summary.json" \
  --pairs "$out_dir/pairs.md" --key "$out_dir/pairs-key.json" || exit 1

rm -rf "$artifacts"
echo "run-spike: done. $out_dir/topology.md has the decision; rate $out_dir/pairs.md blind, then check it against pairs-key.json."
