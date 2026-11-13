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
# Put each file in place by renaming a finished copy over the old one. Writing into a program that is
# running fails ("text file busy") and would leave a half-updated install; a rename never does.
place() {
  cp "$1" "$APP_DIR/.$2.new"
  [ "$3" = x ] && chmod +x "$APP_DIR/.$2.new"
  mv -f "$APP_DIR/.$2.new" "$APP_DIR/$2"
}
place "$HERE/heron" heron x
place "$HERE/heron-desktop" heron-desktop x
place "$HERE/heron.png" heron.png -
place "$HERE/install.sh" install.sh x
ln -sf "$APP_DIR/heron" "$BIN_DIR/heron"
ln -sf "$APP_DIR/heron-desktop" "$BIN_DIR/heron-desktop"
# The launcher's Exec line has its own quoting rules: the path goes in double quotes with " ` $ \
# escaped, a literal % is doubled, and backslashes are doubled once more for the file format. Without
# this a folder name with a space in it would be split into two arguments and Heron would not start.
exec_path=$(printf '%s' "$APP_DIR/heron-desktop" | sed -e 's/[\\"`$]/\\&/g' -e 's/%/%%/g' -e 's/\\/\\\\/g')
{
  printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=Heron' 'Comment=Personal AI agent that runs on your computer'
  printf 'Exec="%s"\n' "$exec_path"
  printf 'Icon=%s\n' "$APP_DIR/heron.png"
  printf '%s\n' 'Terminal=false' 'Categories=Utility;Office;' 'StartupWMClass=heron-desktop'
} > "$DESKTOP_DIR/heron.desktop"
echo "Heron is installed. Open it from your applications menu, or run: heron-desktop"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "To use the 'heron' command in a terminal, add $BIN_DIR to your PATH." ;; esac
if ! ldconfig -p 2>/dev/null | grep -q libwebkit2gtk-4.1; then
  echo "The Heron window needs WebKitGTK 4.1. Install it with your package manager, for example:"
  echo "  sudo apt install libwebkit2gtk-4.1-0     (Debian, Ubuntu)"
  echo "  sudo dnf install webkit2gtk4.1           (Fedora)"
fi
