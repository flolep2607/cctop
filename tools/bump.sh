#!/usr/bin/env bash
# Bumps the versions for a release: the root to the version given, and every
# internal crate tools/release-plan.sh says must move by one patch, with its
# `=` pin, then refreshes Cargo.lock. Prints what it did; commits nothing.
#
#   tools/bump.sh 0.28.4
#
# A convenience, not the only way. By hand a bump is one `version` line per
# bumped crate under crates/, its pin under `[workspace.dependencies]`, the
# root version and Cargo.lock, and `verify / release-plan` checks either the
# same. For a minor or major step of an internal crate, edit its line by hand.
#
# It compares the last tag with HEAD, so commit the changes being released
# first: an edit still in the working tree is not seen.

set -euo pipefail

[ $# = 1 ] || { echo "usage: $0 <new release version>" >&2; exit 2; }
new=$1
root=$(git rev-parse --show-toplevel)
cd "$root"

base=$(tools/release-plan.sh base)
plan=$(tools/release-plan.sh needs-bump)

first_version() { sed -n '0,/^version = "/s/^version = "\([^"]*\)".*/\1/p'; }
set_first_version() { sed -i "0,/^version = \".*\"/s//version = \"$2\"/" "$1"; }

meta=$(cargo metadata --format-version 1 --no-deps --offline)
while IFS=$'\t' read -r name reason; do
    [ -n "$name" ] || continue
    manifest=$(jq -r --arg n "$name" '.packages[] | select(.name == $n) | .manifest_path' <<<"$meta")
    rel=${manifest#"$root"/}
    now=$(first_version <"$manifest")
    was=$(git show "$base:$rel" 2>/dev/null | first_version || true)
    if [ -n "$was" ] && [ "$now" != "$was" ]; then
        echo "$name: already $now (was $was in $base), left alone"
        continue
    fi
    next=$(awk -F. '{print $1 "." $2 "." $3 + 1}' <<<"$now")
    set_first_version "$manifest" "$next"
    sed -i "s/^\($name = .*version = \"=\)[^\"]*\"/\1$next\"/" Cargo.toml
    echo "$name: $now -> $next ($reason)"
done <<<"$plan"

old=$(first_version <Cargo.toml)
set_first_version Cargo.toml "$new"
echo "cctop: $old -> $new"

# The tests run this in a scratch copy on a runner with no registry cache,
# where an offline update has nothing to resolve against; the lock is not what
# they check.
if [ -z "${CCTOP_BUMP_SKIP_LOCK:-}" ]; then
    cargo update --workspace --offline --quiet
    echo "Cargo.lock refreshed"
fi
echo "commit, then tools/release-plan.sh check says what the release publishes"
