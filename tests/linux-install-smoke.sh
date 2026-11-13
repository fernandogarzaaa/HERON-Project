#!/usr/bin/env bash
# Install/uninstall into a throwaway home, including spaces and '%' in the path.
set -euo pipefail
cd "$(dirname "$0")/.."
sh -n packaging/linux/install.sh
bash -n packaging/linux/build.sh

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
home="$tmp/home with spaces %"
package="$tmp/package"
mkdir -p "$home/.heron" "$package"
printf 'keep this memory\n' > "$home/.heron/marker"
printf '#!/bin/sh\nexit 0\n' > "$package/heron"
printf '#!/bin/sh\nexit 0\n' > "$package/heron-desktop"
printf 'icon fixture\n' > "$package/heron.png"
cp packaging/linux/install.sh "$package/install.sh"
chmod +x "$package/heron" "$package/heron-desktop" "$package/install.sh"
export HOME="$home" XDG_DATA_HOME="$home/.local/share"
sh "$package/install.sh" >/dev/null
sh "$package/install.sh" >/dev/null
app="$XDG_DATA_HOME/heron"
test -x "$app/heron" && test -x "$app/heron-desktop"
test -L "$HOME/.local/bin/heron" && test -L "$HOME/.local/bin/heron-desktop"
test -e "$XDG_DATA_HOME/applications/heron.desktop"
grep -Fq 'Exec="' "$XDG_DATA_HOME/applications/heron.desktop"
grep -Fq '%%' "$XDG_DATA_HOME/applications/heron.desktop"
sh "$package/install.sh" --uninstall >/dev/null
test ! -e "$app/heron" && test ! -e "$HOME/.local/bin/heron"
test -f "$HOME/.heron/marker"
printf 'Linux installer smoke test: OK\n'
