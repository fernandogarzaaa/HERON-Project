#!/usr/bin/env bash
# Builds dist/Heron-linux-x64.tar.gz. Needs the Slint system libraries:
# libfontconfig-dev, libxkbcommon-dev, libxkbcommon-x11-dev,
# libxcb-shape0-dev, libxcb-xfixes0-dev, libx11-dev, libx11-xcb-dev,
# libxcursor-dev, libxi-dev, libssl-dev and pkg-config (see
# packaging/slint-packaging-notes.md).
# Run from the repository root:  ./packaging/linux/build.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release --features desktop --bins
OUT=dist/Heron-linux-x64
rm -rf "$OUT" dist/Heron-linux-x64.tar.gz
mkdir -p "$OUT"
cp target/release/heron-desktop target/release/heron "$OUT/"
cp packaging/icons/256x256.png "$OUT/heron.png"
cp packaging/linux/install.sh "$OUT/install.sh"
cp LICENSE "$OUT/LICENSE"
chmod +x "$OUT/install.sh"
tar -C dist -czf dist/Heron-linux-x64.tar.gz Heron-linux-x64
echo "Built dist/Heron-linux-x64.tar.gz"
