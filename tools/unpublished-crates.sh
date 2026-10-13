#!/usr/bin/env bash
# The workspace crates whose current version is not on crates.io yet, one per
# line, in the order they must be published: each after the internal crates it
# depends on. What release.yml's `publish-crate` publishes.
#
#   tools/unpublished-crates.sh
#
# A release bumps only the crates that changed (release-plz decides which; see
# release-plz.toml), so the rest are on crates.io at their version already and
# are skipped with a notice. The same skip makes a publish that failed part-way
# safe to re-run as it is.
#
# Any answer from the index but "here" or "no such crate" is an error, so that
# a network failure cannot pass for "unpublished" and send a crate to be
# published twice, or for "published" and skip one.
#
# It never runs `cargo publish` in any form. It needs cargo, jq and curl.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

die() {
    echo "::error::$*" >&2
    exit 1
}

# name -> {version, internal: [path dependencies]}, for every workspace package.
meta=$(cargo metadata --format-version 1 --no-deps --offline |
    jq -c '[.packages[] | {key: .name, value: {
        version,
        internal: [.dependencies[] | select(.path != null) | .name] | unique,
    }}] | from_entries')

# Each package after the ones it depends on. Derived rather than listed, so a
# new crate needs no edit here: cctop-tunnel comes out first because core
# depends on it, and the root last.
order=$(jq -r '
    . as $m
    | def visit($n; $seen):
        if ($seen | index($n)) then $seen
        else (reduce $m[$n].internal[] as $d ($seen; visit($d; .))) + [$n]
        end;
      reduce (keys[]) as $n ([]; visit($n; .)) | .[]' <<<"$meta")

# The sparse index's path for a crate name, as crates.io lays it out.
index_path() {
    local n=${1,,}
    case ${#n} in
        1) echo "1/$n" ;;
        2) echo "2/$n" ;;
        3) echo "3/${n:0:1}/$n" ;;
        *) echo "${n:0:2}/${n:2:2}/$n" ;;
    esac
}

# Whether name@version is on crates.io.
on_crates_io() {
    local body status
    body=$(mktemp)
    status=$(curl -sS -o "$body" -w '%{http_code}' "https://index.crates.io/$(index_path "$1")") ||
        { rm -f "$body"; die "could not reach the crates.io index for $1"; }
    case $status in
        # Slurped: `-e` on a stream answers for its last line only, and jq
        # 1.6 says "no" there even when an earlier line matched.
        200) jq -s -e --arg v "$2" 'any(.[]; .vers == $v)' "$body" >/dev/null && { rm -f "$body"; return 0; } ;;
        404) ;;
        *) rm -f "$body"; die "the crates.io index answered $status for $1" ;;
    esac
    rm -f "$body"
    return 1
}

for name in $order; do
    v=$(jq -r --arg n "$name" '.[$n].version' <<<"$meta")
    if on_crates_io "$name" "$v"; then
        echo "::notice::$name@$v is on crates.io already; skipping it" >&2
    else
        echo "$name"
    fi
done
