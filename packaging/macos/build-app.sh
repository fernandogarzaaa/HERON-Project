#!/usr/bin/env bash
# Builds dist/Heron.app (universal: Apple silicon + Intel) and dist/Heron-macos-universal.dmg.
# Run on macOS from the repository root:  ./packaging/macos/build-app.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
rustup target add aarch64-apple-darwin x86_64-apple-darwin
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --features desktop --bins --target "$target"
done
APP=dist/Heron.app
DMG=dist/Heron-macos-universal.dmg
STAGE=dist/dmg
rm -rf "$APP" "$DMG" "$STAGE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/Heron" target/aarch64-apple-darwin/release/heron-desktop target/x86_64-apple-darwin/release/heron-desktop
# The CLI ships as heron-cli, not heron: macOS uses a case-insensitive
# filesystem by default, so Heron and heron are the same file and the second
# lipo silently overwrote the desktop binary with the CLI (v0.3.0 shipped a
# bundle whose only executable was the CLI, so double-clicking showed
# nothing). This matches the Windows name heron-cli.exe.
lipo -create -output "$APP/Contents/MacOS/heron-cli" target/aarch64-apple-darwin/release/heron target/x86_64-apple-darwin/release/heron
# Guard the collision above: the two executables must be different files with
# different content, or the bundle is broken the same way again.
cmp -s "$APP/Contents/MacOS/Heron" "$APP/Contents/MacOS/heron-cli" && { echo "Heron and heron-cli are identical; the desktop binary was overwritten" >&2; exit 1; }
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
cp packaging/icons/icon.icns "$APP/Contents/Resources/icon.icns"
# Ad-hoc signature so the app runs on Apple silicon. For distribution without Gatekeeper warnings,
# sign with a Developer ID certificate and notarize instead (set CODESIGN_IDENTITY).
codesign --force --deep --sign "${CODESIGN_IDENTITY:--}" "$APP"
# Disk image with the usual "drag the app onto Applications" layout.
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Heron $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"
echo "Built $APP and $DMG"
