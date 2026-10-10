#!/usr/bin/env bash
# Pushes GitHub's latest release to the mirror the updater falls back on
# (worker/src/index.js). Run by the release workflow once a release is public,
# and again whenever a release's notes are edited, so the mirror says what
# GitHub says. Needs GH_TOKEN, GH_REPO and RELEASE_MIRROR_TOKEN; without the
# last it says so and succeeds, since a release must not fail over its mirror.
set -euo pipefail

: "${RELEASE_MIRROR_URL:=https://cctop-releases.ecorsiste.workers.dev}"
if [ -z "${RELEASE_MIRROR_TOKEN:-}" ]; then
    echo "::notice::RELEASE_MIRROR_TOKEN is not set; the release mirror was not updated"
    exit 0
fi

gh api "repos/$GH_REPO/releases/latest" |
    curl --fail-with-body --silent --show-error -X PUT \
        -H "Authorization: Bearer $RELEASE_MIRROR_TOKEN" \
        -H "Content-Type: application/json" \
        --data-binary @- "$RELEASE_MIRROR_URL/releases/latest"
