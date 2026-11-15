#!/bin/sh
#   scripts/deploy.sh             server and web client
#   scripts/deploy.sh server      the game server only
#   scripts/deploy.sh web         the web client only
#   scripts/deploy.sh rollback    put the previous server binary back
#
#   PUYO_HOST=user@host           the VM (default ubuntu@puyo.priax.org)
#   PUYO_SITE=https://host        where the game is served (default https://<VM>)
set -eu

HOST=${PUYO_HOST:-ubuntu@puyo.priax.org}
SITE=${PUYO_SITE:-https://${HOST#*@}}
VM_REPO=puyorust
VM_DIR=/opt/puyorust

ROOT=$(cd "$(dirname "$0")/.." && pwd)

step() {
    printf '\n==> %s\n' "$*"
}

die() {
    echo "deploy: $*" >&2
    exit 1
}

on_vm() {
    ssh -o ServerAliveInterval=30 -o ServerAliveCountMax=4 "$HOST" "$@"
}

http_status() {
    curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$@" || true
}

pushed_commit() {
    [ -z "$(git -C "$ROOT" status --porcelain)" ] || die "uncommitted changes, commit or stash them first"
    git -C "$ROOT" fetch --quiet origin master
    sha=$(git -C "$ROOT" rev-parse HEAD)
    [ "$sha" = "$(git -C "$ROOT" rev-parse origin/master)" ] || die "HEAD is not origin/master, push first"
    echo "$sha"
}

build_web() {
    step "Building the web client"
    (cd "$ROOT/client" && trunk build --release)
}

build_server() {
    sha=$1
    step "Building the server on $HOST ($sha)"
    on_vm "set -eu
        . ~/.cargo/env
        cd $VM_REPO
        git pull --ff-only --quiet
        [ \"\$(git rev-parse HEAD)\" = $sha ] || { echo 'the VM did not end up on $sha' >&2; exit 1; }
        nice -n 19 cargo build --release -p server"
}

install_server() {
    step "Restarting the server (waits for running games to finish)"
    on_vm "set -eu
        systemctl cat puyo | grep -q '^TimeoutStopSec=' \
            || echo 'warning: puyo.service has no TimeoutStopSec, systemd will kill the server after 90 s (see DEPLOY.md)' >&2
        sudo install -m 755 $VM_REPO/target/release/server $VM_DIR/server.new
        sudo cmp -s $VM_DIR/server.new $VM_DIR/server || sudo cp -p $VM_DIR/server $VM_DIR/server.prev
        sudo mv $VM_DIR/server.new $VM_DIR/server"
    restart_server
}

restart_server() {
    status=0
    on_vm "sudo systemctl restart puyo" || status=$?
    # ssh losing connection
    if [ "$status" = 255 ]; then
        die "connection lost during the restart. Once 'systemctl is-active puyo' says active on the VM, run '$0 web' if the web client was part of this deploy"
    fi
    check_server
}

check_server() {
    if ! on_vm "sleep 2; systemctl is-active --quiet puyo"; then
        on_vm "journalctl -u puyo -n 30 --no-pager" >&2
        die "the server did not come back up; '$0 rollback' puts the previous binary back"
    fi

    # WebSocket handshake
    status=$(http_status --http1.1 \
        -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
        -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' \
        "$SITE/ws")
    [ "$status" = 101 ] || die "$SITE/ws answered $status instead of 101"
    echo "Server up, $SITE/ws answers"

    if curl -s --max-time 10 "$SITE/api/transport" | grep -q '"url"'; then
        echo "WebTransport offered"
    else
        echo "WebTransport not offered, games go over the WebSocket (see DEPLOY.md §14)"
    fi
}

upload_web() {
    step "Uploading the web client"
    rsync -a --delete --rsync-path='sudo rsync' -e 'ssh -o ServerAliveInterval=30' \
        "$ROOT/client/dist/" "$HOST:$VM_DIR/web/" ||
        die "upload failed, the site still serves the previous web client: run '$0 web' again"
    status=$(http_status "$SITE/")
    [ "$status" = 200 ] || die "$SITE/ answered $status instead of 200"
    echo "Web client up at $SITE/"
}

rollback_server() {
    step "Putting the previous server back"
    on_vm "set -eu
        sudo cp -p $VM_DIR/server.prev $VM_DIR/server.new
        sudo mv $VM_DIR/server.new $VM_DIR/server"
    restart_server
}

case ${1:-all} in
all)
    sha=$(pushed_commit)
    build_web
    build_server "$sha"
    install_server
    upload_web
    ;;
server)
    sha=$(pushed_commit)
    build_server "$sha"
    install_server
    ;;
web)
    pushed_commit >/dev/null
    build_web
    upload_web
    ;;
rollback)
    rollback_server
    ;;
*)
    echo "usage: $0 [all | server | web | rollback]" >&2
    exit 2
    ;;
esac
