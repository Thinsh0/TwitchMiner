#!/usr/bin/env bash
# Signs manifest/twitch.json with the maintainer's offline ed25519 key (not the
# release key) and writes manifest/twitch.json.sig (64 raw bytes, like the release
# signatures). Runs in Git Bash (Git for Windows), Linux and macOS: OpenSSL 3, tail,
# od, tr, grep, sed only.
#
#   tools/sign-manifest.sh <private-key.pem>        sign (after every manifest change)
#   tools/sign-manifest.sh --generate <new-key.pem> make the key pair, once
#
# OPENSSL=/path/to/openssl overrides the openssl used (Git for Windows ships one in
# "C:\Program Files\Git\usr\bin\openssl.exe", i.e. /usr/bin/openssl in Git Bash).
# See docs/DEVELOPMENT.md, « Signer le manifeste Twitch ».
set -euo pipefail

OPENSSL=${OPENSSL:-openssl}
root=$(cd "$(dirname "$0")/.." && pwd)
manifest="$root/manifest/twitch.json"
signature="$manifest.sig"
source_file="$root/crates/twitch/src/manifest.rs"

die() {
    echo "error: $*" >&2
    exit 1
}

usage() {
    sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
}

# Absolute path of an existing directory's file, without readlink -f.
absolute() {
    local dir
    dir=$(cd "$(dirname "$1")" 2>/dev/null && pwd) || die "no such directory: $(dirname "$1")"
    echo "$dir/$(basename "$1")"
}

inside_repo() {
    case "$(absolute "$1")" in
        "$root"/*) return 0 ;;
        *) return 1 ;;
    esac
}

# The 32 raw bytes of the public key, in hex (64 characters).
public_hex() {
    "$OPENSSL" pkey -in "$1" -pubout -outform DER | tail -c 32 | od -An -v -tx1 | tr -d ' \n\r'
}

# MANIFEST_PUBLIC_KEY as written in the source (empty when not set yet).
built_in_hex() {
    grep -o 'MANIFEST_PUBLIC_KEY: &str = "[0-9a-fA-F]*"' "$source_file" | sed 's/.*"\(.*\)"/\1/' | tr 'A-F' 'a-f'
}

command -v "$OPENSSL" >/dev/null 2>&1 \
    || die "openssl not found; in Git Bash it is /usr/bin/openssl, or set OPENSSL=/path/to/openssl"

generate() {
    local out=$1
    [ -e "$out" ] && die "$out already exists: not overwritten"
    inside_repo "$out" && die "keep the private key outside the repository (it must never be committed)"
    (umask 077 && "$OPENSSL" genpkey -algorithm ed25519 -out "$out") \
        || die "openssl cannot make an ed25519 key (OpenSSL 1.1.1 or later needed)"
    chmod 600 "$out" 2>/dev/null || true
    local hex
    hex=$(public_hex "$out")
    [ ${#hex} -eq 64 ] || die "unexpected public key: $hex"
    cat <<MSG
Private key written to $out.
Keep it offline (and a backup copy): never in the repository, never in a CI secret.
If it is lost, the builds that carry its public key can no longer take manifest
updates; a new key needs a new release.

Public key, to paste into MANIFEST_PUBLIC_KEY (crates/twitch/src/manifest.rs):
$hex

Then sign the manifest: tools/sign-manifest.sh $out
MSG
}

sign() {
    local key=$1
    [ -f "$key" ] || die "no such key file: $key"
    inside_repo "$key" && die "the private key is inside the repository: move it out (it must never be committed)"
    [ -f "$manifest" ] || die "missing $manifest"
    if grep -q $'\r' "$manifest"; then
        die "manifest/twitch.json has Windows line endings (CRLF): the signed bytes would not be the published ones. Convert it to LF (git checkout -- manifest/twitch.json, or set the editor to LF)"
    fi

    local hex expected
    hex=$(public_hex "$key") || die "$key is not a private key openssl can read"
    [ ${#hex} -eq 64 ] || die "$key is not an ed25519 key"
    expected=$(built_in_hex)
    if [ -n "$expected" ] && [ "$expected" != "$hex" ]; then
        die "this key does not match MANIFEST_PUBLIC_KEY ($expected): builds would refuse the signature"
    fi

    tmp=$(mktemp -d)
    trap 'rm -rf "${tmp:-}"' EXIT
    "$OPENSSL" pkeyutl -sign -inkey "$key" -rawin -in "$manifest" -out "$tmp/twitch.json.sig" \
        || die "signing failed (OpenSSL 3 needed for -rawin)"
    local size
    size=$(wc -c <"$tmp/twitch.json.sig" | tr -d ' \r')
    [ "$size" -eq 64 ] || die "unexpected signature size: $size bytes"

    # Read back and check with the public key derived from the private one.
    "$OPENSSL" pkey -in "$key" -pubout -out "$tmp/public.pem"
    "$OPENSSL" pkeyutl -verify -pubin -inkey "$tmp/public.pem" -rawin \
        -in "$manifest" -sigfile "$tmp/twitch.json.sig" >/dev/null \
        || die "the new signature does not verify: nothing written"
    cp "$tmp/twitch.json.sig" "$signature"
    "$OPENSSL" pkeyutl -verify -pubin -inkey "$tmp/public.pem" -rawin \
        -in "$manifest" -sigfile "$signature" >/dev/null \
        || die "$signature does not verify after writing"

    local revision
    revision=$(grep -m1 '"revision"' "$manifest" | tr -dc '0-9')
    echo "Signed manifest/twitch.json (revision $revision) -> manifest/twitch.json.sig (verified)."
    echo "Public key: $hex"
    if [ -z "$expected" ]; then
        echo "MANIFEST_PUBLIC_KEY is still empty: paste the public key above into crates/twitch/src/manifest.rs."
    fi
    echo "Did you increment \"revision\"? Commit manifest/twitch.json and manifest/twitch.json.sig together."
}

case "${1:-}" in
    --generate) [ $# -eq 2 ] || usage; generate "$2" ;;
    -h | --help | "") usage ;;
    -*) usage ;;
    *) [ $# -eq 1 ] || usage; sign "$1" ;;
esac
