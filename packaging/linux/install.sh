#!/usr/bin/env sh
# Installs Heron for the current user from an unpacked Heron-linux-x64 folder. No root needed.
#   ./install.sh            install        ./install.sh --uninstall   remove (your data in ~/.heron is kept)
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/heron"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

if [ "${1:-}" = "--uninstall" ]; then
  rm -rf "$APP_DIR" "$BIN_DIR/heron" "$BIN_DIR/heron-desktop" "$DESKTOP_DIR/heron.desktop"
  echo "Heron was removed. Your conversations and memory are still in ~/.heron; delete that folder to remove them too."
  exit 0
fi

mkdir -p "$APP_DIR" "$BIN_DIR" "$DESKTOP_DIR"
cp "$HERE/heron" "$HERE/heron-desktop" "$HERE/heron.png" "$APP_DIR/"
cp "$HERE/install.sh" "$APP_DIR/install.sh"
chmod +x "$APP_DIR/heron" "$APP_DIR/heron-desktop" "$APP_DIR/install.sh"
ln -sf "$APP_DIR/heron" "$BIN_DIR/heron"
ln -sf "$APP_DIR/heron-desktop" "$BIN_DIR/heron-desktop"
cat > "$DESKTOP_DIR/heron.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Heron
Comment=Personal AI agent that runs on your computer
Exec=$APP_DIR/heron-desktop
Icon=$APP_DIR/heron.png
Terminal=false
Categories=Utility;Office;
StartupWMClass=heron-desktop
DESKTOP
echo "Heron is installed. Open it from your applications menu, or run: heron-desktop"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "To use the 'heron' command in a terminal, add $BIN_DIR to your PATH." ;; esac
if ! ldconfig -p 2>/dev/null | grep -q libwebkit2gtk-4.1; then
  echo "The Heron window needs WebKitGTK 4.1. Install it with your package manager, for example:"
  echo "  sudo apt install libwebkit2gtk-4.1-0     (Debian, Ubuntu)"
  echo "  sudo dnf install webkit2gtk4.1           (Fedora)"
fi
