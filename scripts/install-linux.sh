#!/bin/sh
# Installs Rouillo the way a desktop application is expected to be on Linux:
# the binary in $PREFIX/bin, its launcher entry and icons under $PREFIX/share
# (the freedesktop.org layout). The window's app id is the entry's file name,
# which is how the desktop finds the icon to show in its taskbar and menus.
#
#   scripts/install-linux.sh                 install for this user (~/.local)
#   scripts/install-linux.sh uninstall       remove it again
#   PREFIX=/usr DESTDIR=pkg scripts/...      for a package
#   scripts/install-linux.sh install <bin>   install this client binary
#
# Run from the repository it builds the client first; run from a release
# archive it installs the binary shipped next to it.
set -eu

APP_ID=org.priax.Rouillo
BIN_NAME=rouillo

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}
dest=${DESTDIR:-}$prefix
action=${1:-install}

# The launcher and icons: next to this script in a release archive, under
# assets/ in the repository.
if [ -d "$here/share" ]; then
    share=$here/share
else
    share=$here/../assets/linux/share
fi

refresh_caches() {
    # A package manager refreshes these itself.
    [ -z "${DESTDIR:-}" ] || return 0
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$prefix/share/applications" || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f "$prefix/share/icons/hicolor" 2>/dev/null || true
    fi
}

icon_dirs() {
    (cd "$share/icons/hicolor" && ls -d ./*/apps)
}

case $action in
install)
    binary=${2:-}
    if [ -z "$binary" ] && [ -x "$here/$BIN_NAME" ]; then
        binary=$here/$BIN_NAME
    fi
    if [ -z "$binary" ]; then
        (cd "$here/.." && cargo build --release -p client)
        binary=$here/../target/release/client
    fi

    install -Dm755 "$binary" "$dest/bin/$BIN_NAME"
    for dir in $(icon_dirs); do
        install -Dm644 "$share/icons/hicolor/$dir/$APP_ID.png" "$dest/share/icons/hicolor/$dir/$APP_ID.png"
    done
    # The session that starts the launcher may not have $PREFIX/bin in its
    # PATH, so the entry names the binary in full.
    install -d "$dest/share/applications"
    sed "s|^Exec=.*|Exec=$prefix/bin/$BIN_NAME|" "$share/applications/$APP_ID.desktop" \
        >"$dest/share/applications/$APP_ID.desktop"
    chmod 644 "$dest/share/applications/$APP_ID.desktop"
    refresh_caches
    echo "Rouillo installed in $dest"
    ;;
uninstall)
    rm -f "$dest/bin/$BIN_NAME" "$dest/share/applications/$APP_ID.desktop"
    for dir in $(icon_dirs); do
        rm -f "$dest/share/icons/hicolor/$dir/$APP_ID.png"
    done
    refresh_caches
    echo "Rouillo removed from $dest"
    ;;
*)
    echo "usage: $0 [install [binary] | uninstall]" >&2
    exit 2
    ;;
esac
