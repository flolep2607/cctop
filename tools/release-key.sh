#!/usr/bin/env bash
# Makes the ed25519 key pair release archives are signed with, once.
#
# The private half goes straight into the repository secret
# RELEASE_SIGNING_KEY, which tools/sign-release.sh reads in the release
# workflow; it is never printed and the file it was written to is removed. The
# public half is printed, as the line to paste into crates/core/src/update.rs.
#
#   tools/release-key.sh            # refuses if the secret already exists
#   tools/release-key.sh --replace  # makes a new pair anyway
#
# Replacing a key that binaries already carry makes every one of them refuse
# the next release, so --replace is only for a key that never shipped.
# Needs gh (logged in, with admin on the repository) and openssl.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
if [ "${1:-}" != "--replace" ] &&
    gh secret list --json name --jq '.[].name' | grep -qx RELEASE_SIGNING_KEY; then
    echo "RELEASE_SIGNING_KEY is already set; binaries carrying its public half would refuse a new one." >&2
    echo "Run with --replace only if no release has shipped with that key." >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
umask 077
openssl genpkey -algorithm ed25519 -out "$work/key.pem"
gh secret set RELEASE_SIGNING_KEY <"$work/key.pem"
# The DER public key is a fixed 12-byte header and then the 32 raw bytes,
# which are all the updater needs.
public=$(openssl pkey -in "$work/key.pem" -pubout -outform DER | tail -c 32 | base64 -w0)

cat <<EOF
RELEASE_SIGNING_KEY is set. Put this in crates/core/src/update.rs, in place of
the empty key, and release it:

const RELEASE_PUBLIC_KEY: &str = "$public";
EOF
