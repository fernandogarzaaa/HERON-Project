#!/usr/bin/env sh
# Heron installer for macOS and Linux. Downloads the release for your computer, checks it against
# the published checksums, and installs it for the current user.
#
#   curl -fsSL https://raw.githubusercontent.com/fernandogarzaaa/HERON-Project/v0.3.0/scripts/install.sh | sh
#
# Set HERON_VERSION to install a different release (default below).
set -eu
VERSION="${HERON_VERSION:-v0.3.0}"
BASE="${HERON_DOWNLOAD_BASE:-https://github.com/fernandogarzaaa/HERON-Project/releases/download/$VERSION}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

case "$(uname -s)" in
  Darwin) ASSET="Heron-macos-universal.dmg" ;;
  Linux)
    [ "$(uname -m)" = "x86_64" ] || { echo "This release supports 64-bit Intel/AMD Linux only. Build from source for $(uname -m): see the README."; exit 1; }
    ASSET="Heron-linux-x64.tar.gz" ;;
  *) echo "Unsupported system. On Windows, use scripts/install.ps1."; exit 1 ;;
esac

echo "Downloading Heron $VERSION ($ASSET)..."
curl -fL --progress-bar -o "$TMP/$ASSET" "$BASE/$ASSET"
curl -fsSL -o "$TMP/SHA256SUMS.txt" "$BASE/SHA256SUMS.txt"

echo "Checking the download..."
EXPECTED=$(grep " $ASSET\$" "$TMP/SHA256SUMS.txt" | awk '{print $1}')
if command -v sha256sum >/dev/null 2>&1; then ACTUAL=$(sha256sum "$TMP/$ASSET" | awk '{print $1}'); else ACTUAL=$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}'); fi
if [ -z "$EXPECTED" ] || [ "$EXPECTED" != "$ACTUAL" ]; then
  echo "The download does not match the published checksum, so nothing was installed."
  exit 1
fi

if [ "$(uname -s)" = "Darwin" ]; then
  MOUNT="$TMP/mount"
  mkdir -p "$MOUNT"
  hdiutil attach "$TMP/$ASSET" -nobrowse -quiet -mountpoint "$MOUNT"
  DEST="/Applications"
  [ -w "$DEST" ] || { DEST="$HOME/Applications"; mkdir -p "$DEST"; }
  rm -rf "$DEST/Heron.app"
  cp -R "$MOUNT/Heron.app" "$DEST/"
  hdiutil detach "$MOUNT" -quiet
  mkdir -p "$HOME/.local/bin"
  ln -sf "$DEST/Heron.app/Contents/MacOS/heron" "$HOME/.local/bin/heron"
  echo "Heron is installed in $DEST. Open it from Launchpad or Spotlight."
  echo "The 'heron' command is at ~/.local/bin/heron."
else
  tar -xzf "$TMP/$ASSET" -C "$TMP"
  sh "$TMP/Heron-linux-x64/install.sh"
fi
