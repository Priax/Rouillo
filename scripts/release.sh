#!/bin/sh
# Releases a version: bumps it, commits, tags, pushes, waits for GitHub to
# publish the signed game binaries (about 10 min), then deploys.
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

# The game updates itself from the latest GitHub release: deploying a server
# that refuses old clients before that release is out would leave them
# stranded, so the deploy waits for the signed binaries.
wait_for_release() {
    repo=$(git remote get-url origin | sed -E 's#^(git@github.com:|https://github.com/)##; s#\.git$##')
    url="https://api.github.com/repos/$repo/releases/tags/v$version"
    echo "Waiting for GitHub to publish the signed v$version binaries..."
    # Once a minute: GitHub allows 60 unauthenticated API calls an hour.
    for _ in $(seq 45); do
        assets=$(curl -fsS "$url" 2>/dev/null || true)
        if echo "$assets" | grep -q '"rouillo-linux-x86_64.sig"' \
            && echo "$assets" | grep -q '"rouillo-windows-x86_64.exe.sig"'; then
            echo "Release v$version published"
            return 0
        fi
        sleep 60
    done
    die "release v$version not published after 45 min, see the Release workflow; deploy with scripts/deploy.sh once it is"
}

git add Cargo.toml Cargo.lock
git commit -m "v$version"
git tag "v$version"
git push origin master "v$version"

if [ "$deploy" = yes ]; then
    wait_for_release
    "$root/scripts/deploy.sh"
fi
