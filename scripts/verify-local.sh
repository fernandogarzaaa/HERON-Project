#!/usr/bin/env bash
# Local mirror of .github/workflows/ci.yml, step for step, with per-step evidence.
#
# Usage: scripts/verify-local.sh [--strict] [--skip-advisory]
#   --strict          advisory steps (fmt, clippy, audit, deny) also fail the run
#   --skip-advisory   do not run advisory steps
# Environment:
#   EVIDENCE_DIR      where logs and evidence.jsonl go (default target/qualification/<sha>)
#   HERON_TEST_BROWSER  passed through; CI sets 1 (default 1 here too)
#
# Exit status is non-zero when any required step fails. A required step that cannot run on
# this OS is recorded NOT_APPLICABLE with the reason, never PASS.
set -uo pipefail

cd "$(dirname "$0")/.."

strict=0
advisory=1
for arg in "$@"; do
  case "$arg" in
    --strict) strict=1 ;;
    --skip-advisory) advisory=0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

sha=$(git rev-parse HEAD)
tree=$(git rev-parse 'HEAD^{tree}')
dirty=$(git status --porcelain --untracked-files=no | wc -l | tr -d ' ')
os=$(uname -s)
arch=$(uname -m)
platform="$os-$arch"
evidence_dir=${EVIDENCE_DIR:-target/qualification/$sha}
mkdir -p "$evidence_dir"
evidence="$evidence_dir/evidence.jsonl"
: > "$evidence"
export HERON_TEST_BROWSER=${HERON_TEST_BROWSER:-1}

rust=$(rustc --version)
cargo_v=$(cargo --version)
{
  echo "sha=$sha"
  echo "tree=$tree"
  echo "tracked_changes=$dirty"
  echo "platform=$platform"
  echo "uname=$(uname -srm)"
  command -v sw_vers >/dev/null && sw_vers
  echo "rustc=$rust"
  echo "cargo=$cargo_v"
  echo "HERON_TEST_BROWSER=$HERON_TEST_BROWSER"
  echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-target}"
} > "$evidence_dir/environment.txt"

failed_required=0
failed_advisory=0

json_escape() { python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"; }

record() { # gate kind result exit_code started completed log command limitation
  printf '{"gate_id":%s,"kind":"%s","repository":"fernandogarzaaa/HERON-Project","commit_sha":"%s","tree_sha":"%s","tracked_changes":%s,"platform":"%s","rustc":%s,"command":%s,"exit_code":%s,"started_at":"%s","completed_at":"%s","result":"%s","artifact_path":%s,"tests":%s,"limitations":%s}\n' \
    "$(json_escape "$1")" "$2" "$sha" "$tree" "$dirty" "$platform" "$(json_escape "$rust")" \
    "$(json_escape "$8")" "$4" "$5" "$6" "$3" "$(json_escape "$7")" "$(test_counts "$7")" \
    "$( [ -n "${9:-}" ] && echo "[$(json_escape "$9")]" || echo "[]")" >> "$evidence"
}

test_counts() { # sums "test result:" lines in a log
  [ -f "$1" ] || { echo null; return; }
  awk '/^test result:/ { for (i=1;i<=NF;i++) { if ($(i+1) ~ /^(passed|failed|ignored|filtered)/) { k=$(i+1); gsub(/[;,]/,"",k); s[k]+=$i } } }
       END { printf "{\"passed\":%d,\"failed\":%d,\"ignored\":%d,\"filtered\":%d}", s["passed"], s["failed"], s["ignored"], s["filtered"] }' "$1"
}

run_step() { # gate kind command...
  local gate=$1 kind=$2; shift 2
  local log="$evidence_dir/$gate.log"
  local started completed code result
  started=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  echo "==> [$kind] $gate: $*"
  "$@" > "$log" 2>&1
  code=$?
  completed=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  if [ "$code" -eq 0 ]; then result=PASS; else result=FAIL; fi
  echo "    $result (exit $code) log: $log"
  record "$gate" "$kind" "$result" "$code" "$started" "$completed" "$log" "$*"
  if [ "$code" -ne 0 ]; then
    if [ "$kind" = required ]; then failed_required=$((failed_required+1)); else failed_advisory=$((failed_advisory+1)); fi
  fi
}

not_applicable() { # gate kind command reason
  local now; now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  echo "==> [$2] $1: NOT_APPLICABLE ($4)"
  record "$1" "$2" NOT_APPLICABLE null "$now" "$now" "" "$3" "$4"
}

# Required: the steps of the CI "test" job, in order. --no-fail-fast (not in CI) keeps one failing
# test binary from hiding the results of the others.
run_step tests required cargo test --locked --no-fail-fast
if [ "$os" = Darwin ] || [[ "$os" == MINGW* || "$os" == MSYS* ]]; then
  run_step vault-live required cargo test --locked --lib config::tests::connector_vault_live_round_trip -- --ignored --exact
else
  not_applicable vault-live required "cargo test --locked --lib config::tests::connector_vault_live_round_trip -- --ignored --exact" "CI runs the native vault round trip on macOS and Windows only"
fi
run_step desktop-build required cargo build --locked --features desktop --bins
run_step desktop-tests required cargo test --locked --features desktop --bin heron-desktop
run_step ui-snapshot required env HERON_UI_SNAPSHOTS_DIR="$evidence_dir/ui-snapshots" cargo test --locked --features desktop --bin heron-desktop window_layout_tests::main_window_lays_out_and_renders_without_a_binding_loop -- --exact --nocapture
if [ "$os" = Linux ]; then
  run_step linux-install-smoke required bash tests/linux-install-smoke.sh
else
  not_applicable linux-install-smoke required "bash tests/linux-install-smoke.sh" "Linux installer smoke test needs Linux"
fi

# Advisory: checks hosted CI does not run yet.
if [ "$advisory" -eq 1 ]; then
  run_step fmt advisory cargo fmt --all -- --check
  run_step clippy advisory cargo clippy --locked --all-targets --features desktop -- -D warnings
  for tool in audit deny; do
    if cargo "$tool" --version >/dev/null 2>&1; then
      if [ "$tool" = audit ]; then run_step cargo-audit advisory cargo audit; else run_step cargo-deny advisory cargo deny check advisories bans sources; fi
    else
      not_applicable "cargo-$tool" advisory "cargo $tool" "cargo-$tool is not installed"
    fi
  done
fi

echo
echo "sha $sha  platform $platform  evidence $evidence"
echo "required failures: $failed_required  advisory failures: $failed_advisory"
if [ "$failed_required" -gt 0 ]; then exit 1; fi
if [ "$strict" -eq 1 ] && [ "$failed_advisory" -gt 0 ]; then exit 1; fi
exit 0
