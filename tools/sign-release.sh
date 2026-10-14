#!/usr/bin/env bash
# Signs a draft release's archives with ed25519 and attaches each signature as
# `<archive>.sig`: one line of base64 over the archive's 64-byte signature,
# which `fetch_archive` in crates/core/src/update.rs checks before installing.
# Run by the release workflow after every archive is attached and before the
# release is made public, so no updater ever sees it without its signatures.
#
#   tools/sign-release.sh <tag>
#
# Needs GH_TOKEN, GH_REPO and RELEASE_SIGNING_KEY (the PEM private key that
# tools/release-key.sh put in the secret). openssl is the whole toolchain:
# ed25519 needs nothing else, and the runner has it.
#
# Without the key it signs nothing and succeeds, which is the rollout: a
# release whose source carries no public key yet updates over GitHub on TLS
# alone. Once the source does carry one, every binary built from it refuses an
# unsigned release, so then a missing key — or a key that does not match the
# public half in the source — fails here, and the release stays a draft.
set -euo pipefail

tag="${1:?usage: sign-release.sh <tag>}"
public=$(git show "refs/tags/$tag:crates/core/src/update.rs" 2>/dev/null |
    sed -n 's/^const RELEASE_PUBLIC_KEY: &str = "\(.*\)";$/\1/p' || true)

if [ -z "${RELEASE_SIGNING_KEY:-}" ]; then
    if [ -n "$public" ]; then
        echo "::error::$tag carries a release public key, so its binaries refuse unsigned releases, but RELEASE_SIGNING_KEY is not set"
        exit 1
    fi
    echo "::notice::RELEASE_SIGNING_KEY is not set; $tag is published unsigned"
    exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
(
    umask 077
    printf '%s\n' "$RELEASE_SIGNING_KEY" >"$work/key.pem"
)
if [ -n "$public" ]; then
    # The SubjectPublicKeyInfo header for an ed25519 key, then its 32 bytes:
    # the DER openssl wants, from the raw key the updater carries.
    { printf '\x30\x2a\x30\x05\x06\x03\x2b\x65\x70\x03\x21\x00'; base64 -d <<<"$public"; } >"$work/public.der"
fi

gh release download "$tag" --pattern '*.tar.gz' --dir "$work/files"
shopt -s nullglob
archives=("$work"/files/*.tar.gz)
if [ ${#archives[@]} -eq 0 ]; then
    echo "::error::$tag has no archives to sign"
    exit 1
fi
for archive in "${archives[@]}"; do
    openssl pkeyutl -sign -rawin -inkey "$work/key.pem" -in "$archive" -out "$archive.raw"
    if [ -n "$public" ]; then
        # The check the updater will make, made first: a pasted public key that
        # is not this key's half would otherwise reach users as a release none
        # of them can install.
        openssl pkeyutl -verify -rawin -pubin -keyform DER -inkey "$work/public.der" \
            -in "$archive" -sigfile "$archive.raw" >/dev/null
    fi
    base64 -w0 "$archive.raw" >"$archive.sig"
    gh release upload "$tag" "$archive.sig" --clobber
    echo "signed $(basename "$archive")"
done
