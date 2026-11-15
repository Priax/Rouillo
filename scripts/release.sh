#!/bin/sh
# Publishes version X.Y.Z: bumps Cargo.toml and Cargo.lock, commits, tags and
# pushes.
#
#   scripts/release.sh 0.8.35
#   scripts/release.sh 0.8.35 --no-deploy     stop after the push
set -eu

KEEP=4 # number of releases on github
WAIT_TRIES=90

die() {
    echo "release: $*" >&2
    exit 1
}

wait_for_release() {
    repo=$(git remote get-url origin | sed -E 's#^(git@github.com:|https://github.com/)##; s#\.git$##')
    base="https://github.com/$repo/releases/download/v$version"
    echo "Waiting for GitHub to publish the signed v$version binaries..."
    for _ in $(seq $WAIT_TRIES); do
        if curl -fsIo /dev/null "$base/rouillo-linux-x86_64.sig" &&
            curl -fsIo /dev/null "$base/rouillo-windows-x86_64.exe.sig"; then
            echo "Release v$version published"
            return 0
        fi
        sleep 30
    done
    die "release v$version not published after 45 min, see the CI and Release workflows; deploy with scripts/deploy.sh once it is"
}

delete_old_releases() {
    if ! command -v gh >/dev/null; then
        echo "release: gh not found, old releases kept" >&2
        return 0
    fi
    old=$(git tag --sort=-v:refname | tail -n +$((KEEP + 1)))
    [ -n "$old" ] || return 0
    for tag in $old; do
        gh release delete "$tag" --yes 2>/dev/null || true
        git push --quiet origin --delete "$tag" || echo "release: $tag was already gone from GitHub" >&2
        git tag -d "$tag" >/dev/null
    done
    echo "Kept the last $KEEP releases"
}

version=${1:-}
echo "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || die "usage: scripts/release.sh X.Y.Z [--no-deploy]"
case ${2:-} in
'') deploy=yes ;;
--no-deploy) deploy=no ;;
*) die "unknown option: $2" ;;
esac

cd "$(git rev-parse --show-toplevel)"
[ -z "$(git status --porcelain)" ] || die "uncommitted changes, commit or stash them first"
if git rev-parse -q --verify "refs/tags/v$version" >/dev/null; then
    die "tag v$version already exists"
fi

sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
grep -q "^version = \"$version\"" Cargo.toml || die "could not set the version in Cargo.toml"
cargo update --workspace --offline
cargo metadata --locked --format-version 1 >/dev/null || die "Cargo.lock is still out of date"

git add Cargo.toml Cargo.lock
git commit -m "v$version"
git tag "v$version"
git push origin master "v$version"

if [ "$deploy" = yes ]; then
    wait_for_release
    scripts/deploy.sh
    delete_old_releases
fi
