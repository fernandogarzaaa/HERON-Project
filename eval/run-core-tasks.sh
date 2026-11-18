#!/usr/bin/env bash
# Run Heron's core tasks. Usage: bash eval/run-core-tasks.sh --provider <demo|id> --out DIR [options]
# See eval/README.md for the options and exit codes.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 -I "$here/run_core_tasks.py" "$@"
