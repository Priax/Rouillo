#!/bin/sh
# Signs release binaries for the in-game updater (client/src/updater.rs):
# each FILE gets FILE.sig, the Ed25519 signature of "NAME VERSION SHA256" in hex.
#
#   scripts/sign-release.sh KEY.pem VERSION FILE...
set -eu

if [ $# -lt 3 ]; then
    echo "usage: $0 KEY.pem VERSION FILE..." >&2
    exit 2
fi
key=$1
version=$2
shift 2

manifest=$(mktemp)
trap 'rm -f "$manifest"' EXIT

for file in "$@"; do
    name=$(basename "$file")
    hash=$(sha256sum "$file" | cut -d' ' -f1)
    printf '%s %s %s' "$name" "$version" "$hash" >"$manifest"
    openssl pkeyutl -sign -rawin -inkey "$key" -in "$manifest" |
        od -An -v -tx1 | tr -d ' \n' >"$file.sig"
done
