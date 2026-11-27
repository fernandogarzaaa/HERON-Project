#!/usr/bin/env bash
# Builds dist/Heron.app (universal: Apple silicon + Intel) and dist/Heron-macos-universal.dmg.
# Run on macOS from the repository root:
#
#   ./packaging/macos/build-app.sh [--sign | --unsigned]
#
#   --unsigned  ad-hoc signature only (Apple silicon needs one to run). No identity, no notarization.
#   --sign      Developer ID signature with the hardened runtime, then notarize and staple the app
#               and the DMG. Needs CODESIGN_IDENTITY, ASC_KEY_PATH, ASC_KEY_ID and ASC_ISSUER_ID.
#   (neither)   today's behaviour: sign with ${CODESIGN_IDENTITY:--}, no notarization.
#
# Contents/MacOS/Heron is the SwiftUI app (macos/HeronMac); Contents/MacOS/heron-cli is the Rust core.
set -euo pipefail
cd "$(dirname "$0")/../.."

MODE=default
case "${1:-}" in
  "") ;;
  --sign) MODE=sign ;;
  --unsigned) MODE=unsigned ;;
  *) echo "usage: build-app.sh [--sign | --unsigned]" >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { echo "usage: build-app.sh [--sign | --unsigned]" >&2; exit 2; }

if [ "$MODE" = sign ]; then
  # Fail before the long build, not after it.
  missing=
  for var in CODESIGN_IDENTITY ASC_KEY_PATH ASC_KEY_ID ASC_ISSUER_ID; do
    [ -n "${!var:-}" ] || missing="$missing $var"
  done
  if [ -n "$missing" ]; then
    echo "--sign needs these environment variables:$missing" >&2
    exit 2
  fi
  [ -f "$ASC_KEY_PATH" ] || { echo "ASC_KEY_PATH ($ASC_KEY_PATH) is not a file" >&2; exit 2; }
fi

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
TARGETS="aarch64-apple-darwin x86_64-apple-darwin"

# 1. The core. --features desktop is not set, so Slint isn't built.
installed=$(rustup target list --installed)
for target in $TARGETS; do
  case "$installed" in
    *"$target"*) ;;
    *) rustup target add "$target" ;;
  esac
done
for target in $TARGETS; do
  MACOSX_DEPLOYMENT_TARGET=14.0 cargo build --locked --release --bins --target "$target"
done

# 2. The app. One universal binary from SwiftPM; the path comes from --show-bin-path.
SWIFT_ARGS=(-c release --package-path macos/HeronMac --arch arm64 --arch x86_64)
swift build "${SWIFT_ARGS[@]}"
SWIFT_BIN_DIR=$(swift build "${SWIFT_ARGS[@]}" --show-bin-path)
SWIFT_BIN="$SWIFT_BIN_DIR/Heron"
[ -x "$SWIFT_BIN" ] || { echo "SwiftPM built no executable at $SWIFT_BIN" >&2; exit 1; }

# 3. The bundle.
APP=dist/Heron.app
DMG=dist/Heron-macos-universal.dmg
STAGE=dist/dmg
ZIP=dist/Heron-notarize.zip
rm -rf "$APP" "$DMG" "$STAGE" "$ZIP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$SWIFT_BIN" "$APP/Contents/MacOS/Heron"
# The CLI ships as heron-cli, not heron: macOS uses a case-insensitive
# filesystem by default, so Heron and heron are the same file and the second
# copy would silently overwrite the app with the CLI (v0.3.0 shipped a
# bundle whose only executable was the CLI, so double-clicking showed
# nothing). This matches the Windows name heron-cli.exe.
lipo -create -output "$APP/Contents/MacOS/heron-cli" target/aarch64-apple-darwin/release/heron target/x86_64-apple-darwin/release/heron
# Guard the collision above: the two executables must be different files with
# different content, or the bundle is broken the same way again.
if cmp -s "$APP/Contents/MacOS/Heron" "$APP/Contents/MacOS/heron-cli"; then
  echo "Heron and heron-cli are identical; the app binary was overwritten" >&2
  exit 1
fi
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
cp packaging/icons/icon.icns "$APP/Contents/Resources/icon.icns"

# 4. Signing. heron-cli first, then the app that contains it. No --deep: each piece is signed on purpose.
case "$MODE" in
  unsigned)
    # Ad-hoc, so the app runs on Apple silicon.
    codesign --force --sign - "$APP/Contents/MacOS/heron-cli"
    codesign --force --sign - "$APP"
    ;;
  sign)
    # Outside the App Sandbox, spawning children, sandbox-exec and loopback networking need no entitlement.
    # The hardened runtime does need one for Calendar access (EventKit, "Use your Mac's Calendar"); without it
    # macOS never shows the permission prompt. It goes on the app only, not on the core binary.
    # Notarization is where the rest is confirmed.
    codesign --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$APP/Contents/MacOS/heron-cli"
    codesign --force --options runtime --timestamp --entitlements packaging/macos/Heron.entitlements --sign "$CODESIGN_IDENTITY" "$APP"
    ;;
  *)
    codesign --force --deep --sign "${CODESIGN_IDENTITY:--}" "$APP"
    ;;
esac

notarize() {
  # $1: a file notarytool accepts (zip, dmg or pkg)
  xcrun notarytool submit "$1" --key "$ASC_KEY_PATH" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER_ID" --wait
}

# 5. Notarize the app (--sign only), then staple the ticket to it.
if [ "$MODE" = sign ]; then
  ditto -c -k --keepParent "$APP" "$ZIP"
  notarize "$ZIP"
  rm -f "$ZIP"
  xcrun stapler staple "$APP"
fi

# 6. Disk image with the usual "drag the app onto Applications" layout.
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Heron $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"
if [ "$MODE" = sign ]; then
  codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$DMG"
  notarize "$DMG"
  xcrun stapler staple "$DMG"
fi
echo "Built $APP and $DMG"
