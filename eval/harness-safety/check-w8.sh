#!/usr/bin/env bash
# bash eval/harness-safety/check-w8.sh default|stronger
set -euo pipefail
case "${1:-}" in
  default)  want='SandboxSettingsTests/test_(sandboxSettingsShowsChangedToolchain|trustToolchainCallsCore|egressHostListRoundTrips)$'; n=3 ;;
  stronger) want='SandboxSettingsTests/test_egressHostListRoundTrips$'; n=1 ;;
  *) echo "usage: check-w8.sh default|stronger" >&2; exit 2 ;;
esac
listed="$(swift test --package-path macos/HeronMac list)"
test "$(grep -cE "$want" <<<"$listed")" -eq "$n"
swift test --package-path macos/HeronMac --filter SandboxSettingsTests
