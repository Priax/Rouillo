#!/bin/sh
#   scripts/install-linux.sh                    install for this user (~/.local)
#   scripts/install-linux.sh install <binary>   install this client binary
#   scripts/install-linux.sh uninstall          remove it again
#   PREFIX=/usr DESTDIR=pkg scripts/...         for a package

set -eu

APP_ID=org.priax.Rouillo
BIN_NAME=rouillo

here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}
dest=${DESTDIR:-}$prefix

if [ -d "$here/share" ]; then
    share=$here/share
else
    share=$here/../assets/linux/share
fi
icons=$share/icons/hicolor
desktop_file=share/applications/$APP_ID.desktop

icon_sizes() {
    (cd "$icons" && ls -d ./*/apps)
}

refresh_caches() {
    [ -z "${DESTDIR:-}" ] || return 0
    if command -v update-desktop-database >/dev/null; then
        update-desktop-database -q "$prefix/share/applications" || true
    fi
    if command -v gtk-update-icon-cache >/dev/null; then
        gtk-update-icon-cache -q -t -f "$prefix/share/icons/hicolor" 2>/dev/null || true
    fi
}

find_binary() {
    if [ -n "${1:-}" ]; then
        echo "$1"
    elif [ -x "$here/$BIN_NAME" ]; then
        echo "$here/$BIN_NAME"
    else
        (cd "$here/.." && cargo build --release -p client >&2)
        echo "$here/../target/release/client"
    fi
}

install_client() {
    binary=$(find_binary "${1:-}")
    install -Dm755 "$binary" "$dest/bin/$BIN_NAME"
    for size in $(icon_sizes); do
        install -Dm644 "$icons/$size/$APP_ID.png" "$dest/share/icons/hicolor/$size/$APP_ID.png"
    done
    install -d "$dest/share/applications"
    sed "s|^Exec=.*|Exec=$prefix/bin/$BIN_NAME|" "$share/applications/$APP_ID.desktop" >"$dest/$desktop_file"
    chmod 644 "$dest/$desktop_file"
    refresh_caches
    echo "Rouillo installed in $dest"
}

uninstall_client() {
    rm -f "$dest/bin/$BIN_NAME" "$dest/$desktop_file"
    for size in $(icon_sizes); do
        rm -f "$dest/share/icons/hicolor/$size/$APP_ID.png"
    done
    refresh_caches
    echo "Rouillo removed from $dest"
}

case ${1:-install} in
install) install_client "${2:-}" ;;
uninstall) uninstall_client ;;
*)
    echo "usage: $0 [install [binary] | uninstall]" >&2
    exit 2
    ;;
esac
