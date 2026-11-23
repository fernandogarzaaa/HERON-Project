#!/usr/bin/env bash
set -euo pipefail
test "$(grep -c '^#### HS-R' docs/engineering/SECURITY_THREAT_MODEL.md)" -ge 15
grep -q 'egress allowlist' docs/cli-providers.md
grep -q 'index.crates.io' docs/cli-providers.md
grep -qE 'export_baseline|OQ1 stronger option' docs/designs/heron-harness2.md
