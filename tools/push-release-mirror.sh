#!/usr/bin/env bash
# Pushes GitHub's latest release, the recent ones cut to tag and notes, and the
# latest release's files (archives, checksums, signatures) to the mirror the
# updater falls back on (worker/src/index.js). Run by the release workflow once
# a release is public, and again whenever a release's notes are edited, so the
# mirror says what GitHub says. Needs GH_TOKEN, GH_REPO and
# RELEASE_MIRROR_TOKEN; without the last it says so and succeeds, since a
# release must not fail over its mirror.
#
# The files are pushed before the JSON that names them, so an updater that
# reads the new release from the mirror finds its files already there. A file
# that will not go
# up is a warning, not a failure: the updater still has GitHub, and a mirror
# file is only ever installed if its signature checks out, so a partial push
# costs a fallback, never a bad install.
set -euo pipefail

: "${RELEASE_MIRROR_URL:=https://cctop-releases.ecorsiste.workers.dev}"
if [ -z "${RELEASE_MIRROR_TOKEN:-}" ]; then
    echo "::notice::RELEASE_MIRROR_TOKEN is not set; the release mirror was not updated"
    exit 0
fi

push() {
    curl --fail-with-body --silent --show-error -X PUT \
        -H "Authorization: Bearer $RELEASE_MIRROR_TOKEN" \
        -H "Content-Type: application/json" \
        --data-binary @- "$RELEASE_MIRROR_URL$1"
}

latest=$(gh api "repos/$GH_REPO/releases/latest")
tag=$(jq -r .tag_name <<<"$latest")

files=$(mktemp -d)
trap 'rm -rf "$files"' EXIT
if gh release download "$tag" --dir "$files"; then
    for file in "$files"/*; do
        name=$(basename "$file")
        # --upload-file sends the content-length R2 needs to take a stream.
        curl --fail-with-body --silent --show-error --upload-file "$file" \
            -H "Authorization: Bearer $RELEASE_MIRROR_TOKEN" \
            "$RELEASE_MIRROR_URL/assets/$tag/$name" ||
            echo "::warning::could not mirror $name for $tag"
    done
else
    echo "::warning::could not download $tag's files to mirror them"
fi

push /releases/latest <<<"$latest"
# The same page the updater asks GitHub for, and only the fields it reads.
gh api "repos/$GH_REPO/releases?per_page=30" --jq 'map({tag_name, body})' | push /releases
