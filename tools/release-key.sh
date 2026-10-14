#!/usr/bin/env bash
# The ed25519 keys release archives are signed with. The updater trusts two,
# RELEASE_PUBLIC_KEY and RELEASE_BACKUP_KEY in crates/core/src/update.rs, and
# accepts a signature by either.
#
#   tools/release-key.sh                    # make the primary, once
#   tools/release-key.sh --replace          # make a new primary anyway
#   tools/release-key.sh --backup           # make the offline backup, once
#   tools/release-key.sh --sign-with-backup <tag>
#
# The primary's private half goes straight into the repository secret
# RELEASE_SIGNING_KEY, which tools/sign-release.sh reads in the release
# workflow; it is never printed and the file it was written to is removed.
#
# The backup's private half never goes near CI. --backup writes it to
# $RELEASE_BACKUP_KEY_FILE (default ~/.config/cctop-release-backup-key.pem,
# mode 600), refusing to overwrite one that exists; keep a copy of that file
# somewhere offline as well, since losing both keys is the one thing nothing
# recovers from. Each mode prints the public half to paste into update.rs.
#
# Rotating the primary out — it leaked, or its secret was lost:
#
#   1. tools/release-key.sh --replace, and paste the new RELEASE_PUBLIC_KEY
#      over the old one, keeping RELEASE_BACKUP_KEY. Release that as usual:
#      the workflow signs it with the new primary, which installed binaries
#      do not trust yet, so for those minutes they refuse it as not matching
#      its signature — a refusal, not a bad install.
#   2. tools/release-key.sh --sign-with-backup v<version>, on the machine
#      holding the backup: it re-signs the release's archives with the
#      backup, which every installed binary trusts, replaces the signatures
#      on GitHub and pushes them to the mirror. From then on old binaries
#      update to one that trusts the new primary and the backup, and no
#      longer the old primary.
#   3. The backup has now been used on a maintainer's machine; make a new one
#      with --backup (after moving the old file aside) and ship it in the next
#      release, signed by the new primary as usual.
#
# Replacing the primary without step 2 makes every installed binary refuse
# every release from then on. Needs gh (logged in, with admin on the
# repository) and openssl.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
backup_file="${RELEASE_BACKUP_KEY_FILE:-$HOME/.config/cctop-release-backup-key.pem}"

# The DER public key is a fixed 12-byte header and then the 32 raw bytes,
# which are all the updater needs.
public_half() {
    openssl pkey -in "$1" -pubout -outform DER | tail -c 32 | base64 -w0
}

case "${1:-}" in
--backup)
    if [ -e "$backup_file" ]; then
        echo "$backup_file already exists; its public half is $(public_half "$backup_file")" >&2
        echo "Move it aside first if a new backup is really meant." >&2
        exit 1
    fi
    mkdir -p "$(dirname "$backup_file")"
    (
        umask 077
        openssl genpkey -algorithm ed25519 -out "$backup_file"
    )
    chmod 600 "$backup_file"
    cat <<EOF
The backup key is in $backup_file. Copy it somewhere offline, never into CI,
and put this in crates/core/src/update.rs:

const RELEASE_BACKUP_KEY: &str = "$(public_half "$backup_file")";
EOF
    exit 0
    ;;
--sign-with-backup)
    tag="${2:?usage: release-key.sh --sign-with-backup <tag>}"
    if [ ! -r "$backup_file" ]; then
        echo "no backup key at $backup_file (set RELEASE_BACKUP_KEY_FILE)" >&2
        exit 1
    fi
    git fetch --quiet origin "refs/tags/$tag:refs/tags/$tag"
    # sign-release.sh checks the signature against the keys the tag trusts
    # before uploading, so a wrong file fails here rather than on GitHub.
    RELEASE_SIGNING_KEY=$(cat "$backup_file") tools/sign-release.sh "$tag"
    # The mirror serves the signatures it was pushed at release time, which
    # are the primary's; the workflow that refreshes it uses the mirror's token,
    # which only CI holds.
    gh workflow run release-mirror.yml
    echo "Signed $tag with the backup key; the mirror is being refreshed."
    exit 0
    ;;
"" | --replace) ;;
*)
    echo "usage: release-key.sh [--replace | --backup | --sign-with-backup <tag>]" >&2
    exit 2
    ;;
esac

if [ "${1:-}" != "--replace" ] &&
    gh secret list --json name --jq '.[].name' | grep -qx RELEASE_SIGNING_KEY; then
    echo "RELEASE_SIGNING_KEY is already set; binaries carrying its public half would refuse a new one." >&2
    echo "Run with --replace only to rotate it (see the top of this script)." >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
umask 077
openssl genpkey -algorithm ed25519 -out "$work/key.pem"
gh secret set RELEASE_SIGNING_KEY <"$work/key.pem"

cat <<EOF
RELEASE_SIGNING_KEY is set. Put this in crates/core/src/update.rs, in place of
the old primary, and release it:

const RELEASE_PUBLIC_KEY: &str = "$(public_half "$work/key.pem")";
EOF
