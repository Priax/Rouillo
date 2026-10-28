#!/bin/sh
#   scripts/release.sh 0.8.35
#   scripts/release.sh 0.8.35 --no-deploy
set -eu

die() {
    echo "release: $*" >&2
    exit 1
}

version=${1:-}
case $version in
    '' | *[!0-9.]* | .* | *. | *..*) die "usage: scripts/release.sh X.Y.Z [--no-deploy]" ;;
esac
deploy=yes
case ${2:-} in
    '') ;;
    --no-deploy) deploy=no ;;
    *) die "unknown option: $2" ;;
esac

root=$(git rev-parse --show-toplevel)
cd "$root"
[ -z "$(git status --porcelain)" ] || die "uncommitted changes, commit or stash them first"
git rev-parse -q --verify "refs/tags/v$version" >/dev/null && die "tag v$version already exists"

sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
grep -q "^version = \"$version\"" Cargo.toml || die "could not set the version in Cargo.toml"
cargo update --workspace --offline
cargo metadata --locked --format-version 1 >/dev/null || die "Cargo.lock is still out of date"

wait_for_release() {
    repo=$(git remote get-url origin | sed -E 's#^(git@github.com:|https://github.com/)##; s#\.git$##')
    base="https://github.com/$repo/releases/download/v$version"
    echo "Waiting for GitHub to publish the signed v$version binaries..."
    for _ in $(seq 90); do
        if curl -fsIo /dev/null "$base/rouillo-linux-x86_64.sig" \
            && curl -fsIo /dev/null "$base/rouillo-windows-x86_64.exe.sig"; then
            echo "Release v$version published"
            return 0
        fi
        sleep 30
    done
    die "release v$version not published after 45 min, see the CI and Release workflows; deploy with scripts/deploy.sh once it is"
}

git add Cargo.toml Cargo.lock
git commit -m "v$version"
git tag "v$version"
git push origin master "v$version"

if [ "$deploy" = yes ]; then
    wait_for_release
    "$root/scripts/deploy.sh"
fi
