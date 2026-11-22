#!/usr/bin/env bash
# Run Heron's core tasks. Usage: bash eval/run-core-tasks.sh --provider <demo|id> --out DIR [options]
# --task-file <file> runs the tasks of another file (e.g. eval/features/<slug>.md) instead of
# eval/core-tasks.md; --tasks t01,... still picks ids from whichever file is read.
# See eval/README.md and eval/features/README.md for the options and exit codes.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 -I "$here/run_core_tasks.py" "$@"
