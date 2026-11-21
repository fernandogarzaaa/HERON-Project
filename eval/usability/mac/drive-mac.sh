#!/usr/bin/env bash
# Drive the Heron Mac app (U4) headlessly for the usability run.
#
#   bash eval/usability/mac/drive-mac.sh --plan        # print what would run; exits 0 even before U4
#   bash eval/usability/mac/drive-mac.sh [--out DIR] [--app PATH/Heron.app] [--skip-tests]
#
# Exit codes: 0 every step ran; 1 a step failed; 2 usage; 3 blocked (U4's package or an app
# bundle is missing, or this shell can't capture the screen). Status of each step is printed as
# "drive-mac: <step>: ok|FAILED|blocked|skipped: <why>".
#
# What it does once macos/HeronMac exists (spec: docs/designs/heron-v1.md item 40-41 and
# docs/designs/heron-mac-visual.md):
#   1. swift test --filter SnapshotTests: U4's own snapshot tests for the risk notice and the
#      approval sheet, light and dark. These are the only pixel-compared screens in v1.
#   2. Launches the bundle's Contents/MacOS/Heron directly (not `open`, so HERON_HOME reaches
#      it) with a disposable HERON_HOME, so the app's own core starts on empty state.
#   3. Walks the screens with the keyboard paths the visual doc defines (first run by its
#      checkbox and button, then Cmd-1 to Cmd-7 and Cmd-,), and for each screen writes a
#      screenshot (screencapture of the window bounds) and an accessibility dump (System Events
#      `entire contents`, each element's role, name, value and enabled state).
#   4. Builds a gallery (gallery.py) and runs `eve read` on the accessibility text as the
#      keyboard/VoiceOver persona, and `eve run`/eve_multimodal_scan on the screenshots when
#      EVE's Playwright surface is installed.
#   5. Quits the app; its core exits when the app's pipe closes.
#
# Needs, outside Claude Code's sandbox: Screen Recording and Accessibility permission for the
# terminal that runs this (System Settings > Privacy & Security). VoiceOver itself is not driven:
# the VoiceOver checks in tasks.md (`mac.keyboard`) are done by a person, using the dumps here as
# the expected labels.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
PKG="$REPO/macos/HeronMac"
OUT=""
APP=""
PLAN=0
SKIP_TESTS=0

usage() { sed -n '2,6p' "$0" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --plan) PLAN=1 ;;
    --out) OUT="${2:?}"; shift ;;
    --app) APP="${2:?}"; shift ;;
    --skip-tests) SKIP_TESTS=1 ;;
    -h|--help) usage ;;
    *) echo "drive-mac: unknown option $1" >&2; usage ;;
  esac
  shift
done

# Screens in walk order: name, then the keystroke that reaches it (System Events syntax).
SCREENS=(
  "first-run-1|"
  "today|cmd 1"
  "chat|cmd 2"
  "inbox|cmd 3"
  "memory|cmd 4"
  "tasks|cmd 5"
  "schedules|cmd 6"
  "activity|cmd 7"
  "settings|cmd ,"
)

say() { echo "drive-mac: $*"; }

if [ "$PLAN" = 1 ]; then
  [ -f "$PKG/Package.swift" ] && state="present" || state="missing (U4 not landed)"
  say "plan: macos/HeronMac is $state"
  say "plan: 1. swift test --package-path macos/HeronMac --filter SnapshotTests"
  say "plan: 2. launch <app>/Contents/MacOS/Heron with HERON_HOME=<out>/home"
  for s in "${SCREENS[@]}"; do say "plan: 3. screen ${s%%|*} via '${s#*|}': screencapture + accessibility dump"; done
  say "plan: 4. gallery.py <out>/shots <out>/gallery --ax <out>/ax; eve read screens.md --persona accessibility-user"
  say "plan: 5. quit the app"
  say "plan: manual: VoiceOver checks from tasks.md mac.keyboard, against <out>/ax"
  exit 0
fi

if [ ! -f "$PKG/Package.swift" ]; then
  say "preflight: blocked: macos/HeronMac/Package.swift is missing (U4 has not landed)"
  exit 3
fi
OUT="${OUT:-$HERE/../out/mac-$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$OUT/shots" "$OUT/ax" "$OUT/home"
status=0

# 1. Snapshot tests
if [ "$SKIP_TESTS" = 1 ]; then
  say "snapshot-tests: skipped: --skip-tests"
elif swift test --package-path "$PKG" --filter SnapshotTests > "$OUT/snapshot-tests.log" 2>&1; then
  say "snapshot-tests: ok"
else
  say "snapshot-tests: FAILED: see $OUT/snapshot-tests.log"; status=1
fi

# 2. Launch
if [ -z "$APP" ]; then
  APP="$(ls -d "$REPO"/dist/*.app "$REPO"/packaging/macos/build/*.app 2>/dev/null | head -n 1 || true)"
fi
if [ -z "$APP" ] || [ ! -x "$APP/Contents/MacOS/Heron" ]; then
  say "launch: blocked: no Heron.app bundle (build one with packaging/macos/build-app.sh --unsigned, or pass --app)"
  exit 3
fi
HERON_HOME="$OUT/home" "$APP/Contents/MacOS/Heron" > "$OUT/app.log" 2>&1 &
APP_PID=$!
trap 'kill "$APP_PID" 2>/dev/null || true' EXIT
sleep 5
if ! kill -0 "$APP_PID" 2>/dev/null; then
  say "launch: FAILED: the app exited; see $OUT/app.log"; exit 1
fi
say "launch: ok (pid $APP_PID)"

ax() { osascript -e "tell application \"System Events\" to tell (first process whose unix id is $APP_PID) to $1"; }

capture() {
  local name="$1" bounds
  if ! bounds="$(ax 'get {position, size} of window 1' 2>"$OUT/ax/$name.err")"; then
    say "screen $name: blocked: no Accessibility permission for this terminal ($(head -c 200 "$OUT/ax/$name.err"))"
    return 3
  fi
  # osascript prints the list as "x, y, w, h"; screencapture -R wants "x,y,w,h".
  local rect; rect="$(echo "$bounds" | tr -d '{} ' )"
  if ! screencapture -x -o -R"$rect" "$OUT/shots/$name.png" 2>"$OUT/shots/$name.err"; then
    say "screen $name: blocked: screencapture failed (Screen Recording permission?)"
    return 3
  fi
  # One element reference per line, e.g. "checkbox I've read these risks ... of window Heron".
  ax 'get entire contents of window 1' | tr ',' '\n' > "$OUT/ax/$name.txt" 2>/dev/null || true
  say "screen $name: ok"
}

for entry in "${SCREENS[@]}"; do
  name="${entry%%|*}"; keys="${entry#*|}"
  if [ "$name" = "first-run-1" ]; then
    capture "$name" || { status=$?; break; }
    # u01's keyboard path: tick the checkbox, then Accept and continue, then step 2 and 3.
    ax 'click checkbox 1 of window 1' >/dev/null 2>&1 || say "first-run: FAILED: no checkbox in window 1"
    ax 'click button "Accept and continue" of window 1' >/dev/null 2>&1 || say "first-run: FAILED: no 'Accept and continue' button"
    sleep 1; capture "first-run-2" || true
    ax 'keystroke return' >/dev/null 2>&1 || true
    sleep 1; capture "first-run-3" || true
    ax 'click button "Start using Heron" of window 1' >/dev/null 2>&1 || say "first-run: FAILED: no 'Start using Heron' button"
    sleep 1
    continue
  fi
  key="${keys#cmd }"
  ax "keystroke \"$key\" using command down" >/dev/null 2>&1 || say "screen $name: FAILED: keystroke cmd-$key"
  sleep 1
  capture "$name" || { status=$?; break; }
done

# 4. Gallery and EVE
if python3 -I "$HERE/gallery.py" "$OUT/shots" "$OUT/gallery" --ax "$OUT/ax"; then
  eve read "$OUT/gallery/screens.md" --genre interface --persona accessibility-user \
    --goal "use every screen with the keyboard and VoiceOver alone" --json --quiet --out "$OUT/eve-read" \
    > "$OUT/eve-read.json" 2>"$OUT/eve-read.err" || true
  say "eve-read: ok ($OUT/eve-read.json)"
  if eve doctor 2>/dev/null | grep -q '✓ playwright'; then
    eve run "file://$OUT/gallery/index.html" --persona accessibility-user --steps 30 --quiet --out "$OUT/eve-run" || true
    say "eve-run: ok ($OUT/eve-run)"
  else
    say "eve-run: skipped: EVE's Playwright surface is not installed (npx playwright install chromium)"
  fi
else
  say "gallery: FAILED: no screenshots"; status=1
fi

# 5. Quit
ax 'keystroke "q" using command down' >/dev/null 2>&1 || true
sleep 2
kill "$APP_PID" 2>/dev/null || true
say "done: $OUT"
exit "$status"
