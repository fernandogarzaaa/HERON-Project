#!/usr/bin/env bash
# Heron v1 usability runner. Replays the recorded fixtures by default (no core, no ports);
# --live starts a core per task. Options: eval/usability/README.md.
set -euo pipefail
exec python3 -I "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/run.py" "$@"
