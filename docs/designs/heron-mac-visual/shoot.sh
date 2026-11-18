#!/bin/bash
# Screenshot mockups into shots/. Usage: bash shoot.sh page[:query][:name] ...
# Needs Google Chrome. Run outside any sandbox that blocks Chrome's Mach ports.
set -euo pipefail
cd "$(dirname "$0")"
CH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
PROF="${TMPDIR:-/tmp}/heron-mock-chrome"
mkdir -p shots
for spec in "$@"; do
  IFS=: read -r page query name <<<"$spec"
  name="${name:-$page}"
  size="1260,840"
  case "$page" in settings) size="800,720";; menubar) size="1260,720";; esac
  "$CH" --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files \
    --user-data-dir="$PROF" --window-size="$size" --virtual-time-budget=1500 \
    --screenshot="$PWD/shots/$name.png" "file://$PWD/$page.html?shot=1&reduce=1&${query:-}" 2>&1 | grep -q written
  echo "shots/$name.png"
done
