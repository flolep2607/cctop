#!/usr/bin/env bash
# Bumps the versions for a release: the root to the version given, and every
# internal crate tools/release-plan.sh says must move — by one patch, or, for
# a crate whose public API broke, by a breaking step (0.28.6 -> 0.29.0) with
# its requirement under `[workspace.dependencies]` moved to match — then
# refreshes Cargo.lock. Prints what it did; commits nothing.
#
#   tools/bump.sh 0.28.4
#
# Which crates broke is cargo-semver-checks' answer, so this needs the version
# rust-toolchain.toml pins, or CCTOP_BREAKING naming them (see
# tools/release-plan.sh).
#
# A convenience, not the only way. By hand a bump is one `version` line per
# bumped crate under crates/, for a breaking one its requirement under
# `[workspace.dependencies]`, the root version and Cargo.lock, and `verify /
# release-plan` checks either the same.
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
while IFS=$'\t' read -r name reason level; do
    [ -n "$name" ] || continue
    manifest=$(jq -r --arg n "$name" '.packages[] | select(.name == $n) | .manifest_path' <<<"$meta")
    rel=${manifest#"$root"/}
    now=$(first_version <"$manifest")
    was=$(git show "$base:$rel" 2>/dev/null | first_version || true)
    if [ -n "$was" ] && [ "$now" != "$was" ]; then
        echo "$name: already $now (was $was in $base), left alone"
        continue
    fi
    if [ "$level" = breaking ]; then
        next=$(awk -F. '{ if ($1 == 0) print "0." $2 + 1 ".0"; else print $1 + 1 ".0.0" }' <<<"$now")
        # The caret on the new version's compatible part, which is what
        # release-plan.sh checks every requirement against.
        req=$(awk -F. '{ if ($1 == 0) print "0." $2; else print $1 }' <<<"$next")
        sed -i "s/^\($name = .*version = \"\)[^\"]*\"/\1$req\"/" Cargo.toml
        echo "$name: $now -> $next, requirement \"$req\" ($reason)"
    else
        next=$(awk -F. '{print $1 "." $2 "." $3 + 1}' <<<"$now")
        echo "$name: $now -> $next ($reason)"
    fi
    set_first_version "$manifest" "$next"
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
