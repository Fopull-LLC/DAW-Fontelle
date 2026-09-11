#!/bin/sh
# Installs Fontelle for the current user — no root, nothing outside $HOME.
#
#   the binary        → ~/.local/bin/fontelle
#   the menu entry    → ~/.local/share/applications/com.fopull.Fontelle.desktop
#   the icon          → ~/.local/share/icons/hicolor/256x256/apps/com.fopull.Fontelle.png
#
# Run it from the folder the release archive unpacked into. Run it again to
# upgrade by hand; Fontelle's own start menu can do the same from inside.
# `./install.sh --uninstall` takes all three away again.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin="${XDG_BIN_HOME:-$HOME/.local/bin}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
apps="$data/applications"
icons="$data/icons/hicolor/256x256/apps"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin/fontelle" "$apps/com.fopull.Fontelle.desktop" "$icons/com.fopull.Fontelle.png"
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" || true
    echo "Fontelle removed. Your projects, soundfonts and settings were left alone."
    exit 0
fi

if [ ! -f "$here/fontelle" ]; then
    echo "install.sh: no 'fontelle' binary next to this script — run it from the unpacked release folder." >&2
    exit 1
fi

# What the binary links that a desktop may not have: lilv (LV2 hosting)
# above all. Said before anything is copied, with the package to install,
# because "error while loading shared libraries" after a successful install
# reads as a broken program.
if command -v ldd >/dev/null 2>&1; then
    missing=$(ldd "$here/fontelle" 2>/dev/null | awk '/not found/ {print $1}')
    if [ -n "$missing" ]; then
        echo "install.sh: fontelle needs libraries this machine does not have:" >&2
        echo "$missing" | sed 's/^/    /' >&2
        echo "  Debian/Ubuntu:  sudo apt install liblilv-0-0 libasound2 libdbus-1-3" >&2
        echo "  Fedora:         sudo dnf install lilv alsa-lib dbus-libs" >&2
        echo "  Arch:           sudo pacman -S lilv alsa-lib dbus" >&2
        echo "Installing anyway; it will run once they are there." >&2
    fi
fi

mkdir -p "$bin" "$apps" "$icons"
install -m 755 "$here/fontelle" "$bin/fontelle"
# The menu entry names the binary by its full path: a desktop session does
# not always have ~/.local/bin on its PATH, and a launcher that cannot find
# the program it names fails silently.
sed "s|^Exec=.*|Exec=$bin/fontelle|" "$here/com.fopull.Fontelle.desktop" > "$apps/com.fopull.Fontelle.desktop"
chmod 644 "$apps/com.fopull.Fontelle.desktop"
install -m 644 "$here/com.fopull.Fontelle.png" "$icons/com.fopull.Fontelle.png"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true

echo "Fontelle installed to $bin/fontelle — it is in your application menu now."
case ":$PATH:" in
    *":$bin:"*) ;;
    *) echo "Note: $bin is not on your PATH; the menu entry works either way." ;;
esac
