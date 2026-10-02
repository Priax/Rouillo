#!/bin/sh
# Releases a version: bumps it, commits, tags, pushes, then deploys.
#
#   scripts/release.sh 0.8.35            everything, deploy included
#   scripts/release.sh 0.8.35 --no-deploy
#
# Cargo.lock is updated here, before the commit: left to the pre-commit hook's
# cargo run, it would change after being staged and block the deploy.
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

git add Cargo.toml Cargo.lock
git commit -m "v$version"
git tag "v$version"
git push origin master "v$version"

[ "$deploy" = yes ] && "$root/scripts/deploy.sh"
