#!/usr/bin/env bash
# Builds dist/Heron-linux-x64.tar.gz. Needs libwebkit2gtk-4.1-dev and libgtk-3-dev.
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
