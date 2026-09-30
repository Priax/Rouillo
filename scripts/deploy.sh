#!/bin/sh
# Deploys Rouillo to its server: the game server, the web client, or both.
#
#   scripts/deploy.sh             both, in an order that keeps them compatible
#   scripts/deploy.sh server      the game server only
#   scripts/deploy.sh web         the web client only
#   scripts/deploy.sh rollback    put the previous server binary back
#
#   PUYO_HOST=user@host           where to deploy (default ubuntu@puyo.priax.org)
#   PUYO_SITE=https://host        where it is served (default: that host, over https)
#
# What gets deployed is the commit checked out here, which must be pushed: the
# server is built on the VM from the repository, the web client here, and the
# two must come from the same source or their protocols may differ.
#
# Restarting does not cut games: on SIGTERM the server stops starting new ones,
# lets the running ones finish and only then exits (see SHUTDOWN_DEADLINE in
# server/src/main.rs), so the restart below can take a few minutes.
set -eu

HOST=${PUYO_HOST:-ubuntu@puyo.priax.org}
SITE=${PUYO_SITE:-https://${HOST#*@}}
REPO='~/puyorust'
DIR=/opt/puyorust

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
root=$here/..
action=${1:-all}

step() {
    printf '\n==> %s\n' "$*"
}

die() {
    echo "deploy: $*" >&2
    exit 1
}

pushed_commit() {
    [ -z "$(git -C "$root" status --porcelain)" ] || die "uncommitted changes, commit or stash them first"
    git -C "$root" fetch --quiet origin master
    sha=$(git -C "$root" rev-parse HEAD)
    [ "$sha" = "$(git -C "$root" rev-parse origin/master)" ] || die "HEAD is not origin/master, push first"
    echo "$sha"
}

build_web() {
    step "Building the web client"
    (cd "$root/client" && trunk build --release)
}

build_server() {
    step "Building the server on $HOST ($1)"
    ssh "$HOST" "set -eu
        . ~/.cargo/env
        cd $REPO
        git pull --ff-only --quiet
        [ \"\$(git rev-parse HEAD)\" = $1 ] || { echo 'the VM did not end up on $1' >&2; exit 1; }
        nice -n 19 cargo build --release -p server"
}

swap_server() {
    step "Restarting the server (waits for running games to finish)"
    ssh "$HOST" "set -eu
        systemctl cat puyo | grep -q '^TimeoutStopSec=' \
            || echo 'warning: puyo.service has no TimeoutStopSec, systemd will kill the server after 90 s (see DEPLOY.md)' >&2
        sudo install -m 755 $REPO/target/release/server $DIR/server.new
        sudo cp -p $DIR/server $DIR/server.prev
        sudo mv $DIR/server.new $DIR/server
        sudo systemctl restart puyo"
    check_server
}

check_server() {
    ssh "$HOST" "sleep 2; systemctl is-active --quiet puyo" || {
        ssh "$HOST" "journalctl -u puyo -n 30 --no-pager" >&2
        die "the server did not come back up; '$0 rollback' puts the previous binary back"
    }
    status=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 --http1.1 \
        -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
        -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' \
        "$SITE/ws" || true)
    [ "$status" = 101 ] || die "$SITE/ws answered $status instead of 101"
    echo "Server up, $SITE/ws answers"
}

push_web() {
    step "Uploading the web client"
    rsync -a --delete --rsync-path='sudo rsync' "$root/client/dist/" "$HOST:$DIR/web/"
    status=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$SITE/" || true)
    [ "$status" = 200 ] || die "$SITE/ answered $status instead of 200"
    echo "Web client up at $SITE/"
}

case $action in
all)
    sha=$(pushed_commit)
    build_web
    build_server "$sha"
    swap_server
    push_web
    ;;
server)
    sha=$(pushed_commit)
    build_server "$sha"
    swap_server
    ;;
web)
    pushed_commit >/dev/null
    build_web
    push_web
    ;;
rollback)
    step "Putting the previous server back"
    ssh "$HOST" "set -eu
        sudo cp -p $DIR/server.prev $DIR/server.new
        sudo mv $DIR/server.new $DIR/server
        sudo systemctl restart puyo"
    check_server
    ;;
*)
    echo "usage: $0 [all | server | web | rollback]" >&2
    exit 2
    ;;
esac
