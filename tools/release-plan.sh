#!/usr/bin/env bash
# Which crates a release has to bump, and which it has to publish.
#
# cctop is four crates — cctop-core, cctop-serve and cctop-ui under crates/,
# and the binary at the root — and each has its own version. The root's is
# the release version and the tag, and every release bumps it. An internal
# crate is bumped only when it changed since the last release tag, where
# "changed" is any of:
#
#   1. a file under its directory changed;
#   2. its packaged manifest changed without one — a `[workspace.dependencies]`
#      entry it uses was bumped, or a `[workspace.package]` field it inherits.
#      Compared as `cargo metadata` sees the package at the tag and at HEAD, so
#      that what counts is what cargo would publish, not how the TOML is laid
#      out;
#   3. an internal crate it depends on was bumped. The `=` pins make this one
#      unavoidable: a published cctop-ui@0.28.3 requires cctop-core =0.28.3, so
#      a cctop that wants core 0.28.4 beside that ui resolves two cores and
#      `cargo install cctop` fails on mismatched types. Rule 2 already sees the
#      pin move, but it is written down on its own so nobody has to work it
#      out again.
#
# Commands:
#
#   check        The guard `verify` runs. On a release commit (the root version
#                differs from the last tag's) it fails, one `::error::` line per
#                crate, when a changed crate kept its version or an unchanged
#                one was bumped, and prints what the release will publish. On
#                any other commit it says so and passes: a feature branch
#                changes crates without bumping them, and that is fine.
#   needs-bump   The internal crates rules 1-3 say must be bumped, closing rule
#                3 over rules 1 and 2 rather than over the versions, so it
#                answers before anything is bumped. `tools/bump.sh` reads it.
#   unpublished  The workspace crates whose current version is not on
#                crates.io yet, in the order they must be published. What
#                `publish-crate` publishes.
#
# It never runs `cargo publish` in any form. It needs git, cargo, jq and curl,
# and a checkout with its tags and history (`fetch-depth: 0` in CI).
#
# CCTOP_RELEASE_BASE overrides the tag to compare against (tests use it).

set -euo pipefail

root=$(git rev-parse --show-toplevel)
cd "$root"

die() {
    echo "::error::$*" >&2
    exit 1
}

# `cargo metadata` of the tree in the current directory, one object per
# workspace package with only what decides what gets published: paths are
# absolute and differ between the tag's copy and HEAD, and the version is
# what the rules are checking, so both are left out.
manifests() {
    cargo metadata --format-version 1 --no-deps --offline --manifest-path "$1/Cargo.toml" |
        jq -c '[.packages[] | {
            name,
            version,
            deps: [.dependencies[] | select(.path == null)] | sort_by(.name, .kind // "", .target // ""),
            pins: [.dependencies[] | select(.path != null) | del(.path)] | sort_by(.name, .kind // ""),
            internal: [.dependencies[] | select(.path != null) | .name] | unique,
            rest: (del(.id, .version, .manifest_path, .source, .dependencies, .targets) +
                   {targets: [.targets[] | del(.src_path)] | sort_by(.name, .kind)}),
        }] | map({key: .name, value: .}) | from_entries'
}

# Internal crates in publishing order: each after the ones it depends on.
# Derived rather than listed, so a fifth crate needs no edit here.
publish_order() {
    jq -r '
        . as $m
        | def visit($n; $seen):
            if ($seen | index($n)) then $seen
            else (reduce $m[$n].internal[] as $d ($seen; visit($d; .))) + [$n]
            end;
          reduce (keys[]) as $n ([]; visit($n; .)) | .[]' <<<"$1"
}

root_name=cctop

head_meta=$(manifests "$root")

last_tag() {
    if [ -n "${CCTOP_RELEASE_BASE:-}" ]; then
        echo "$CCTOP_RELEASE_BASE"
        return
    fi
    if [ "$(git rev-parse --is-shallow-repository)" = true ]; then
        die "shallow checkout: the release guard needs the history and tags (actions/checkout with fetch-depth: 0)"
    fi
    local version
    version=$(jq -r --arg r "$root_name" '.[$r].version' <<<"$head_meta")
    # Excluding the root's own version: a workflow_dispatch re-run of a release
    # checks out the tag itself, and comparing a release with itself would
    # call nothing changed.
    git describe --tags --abbrev=0 --match 'v*' --exclude "v$version" HEAD 2>/dev/null || true
}

# The metadata of the workspace as it was at a tag, read from a copy of that
# tree: cargo needs the manifests and the source layout, not a checkout.
meta_at() {
    local dir
    dir=$(mktemp -d)
    # shellcheck disable=SC2064 # expand now: $dir is local
    trap "rm -rf '$dir'" RETURN
    git archive "$1" | tar -x -C "$dir"
    manifests "$dir"
}

# Rules 1 and 2 for one internal crate: why it changed, or nothing.
own_change() {
    local name=$1 base=$2 base_meta=$3 dir
    dir=$(jq -r --arg n "$name" '.[$n].dir' <<<"$head_dirs")
    if ! git diff --quiet "$base" HEAD -- "$dir/"; then
        echo "$dir/"
        return
    fi
    # The pins on other internal crates leave out their version requirement:
    # that moving is rule 3, and is reported as such by the caller.
    local shape='.[$n] | del(.version) | .pins |= map(del(.req))'
    if [ "$(jq -c --arg n "$name" "$shape" <<<"$head_meta")" != \
        "$(jq -c --arg n "$name" "$shape" <<<"$base_meta")" ]; then
        echo "its packaged manifest"
    fi
}

# Every internal crate rules 1-3 say must be bumped, as "name<TAB>reason".
# Rule 3 is closed over rules 1 and 2, so the answer does not depend on
# whether the bumps have been made yet; `check` adds rule 3 over the actual
# versions on top, for a dependency bumped without cause.
needs_bump() {
    local base=$1 base_meta=$2 name reason dep
    declare -A why=()
    for name in $internal; do
        if ! jq -e --arg n "$name" 'has($n)' <<<"$base_meta" >/dev/null; then
            why[$name]="it is new since $base"
            continue
        fi
        reason=$(own_change "$name" "$base" "$base_meta")
        if [ -n "$reason" ]; then why[$name]=$reason; fi
    done
    # In publishing order, so a dependency's reason is settled before its
    # dependents look at it.
    for name in $internal; do
        [ -n "${why[$name]:-}" ] && continue
        for dep in $(jq -r --arg n "$name" '.[$n].internal[]' <<<"$head_meta"); do
            if [ -n "${why[$dep]:-}" ]; then
                why[$name]="it depends on $dep, which is bumped"
                break
            fi
        done
    done
    for name in $internal; do
        [ -n "${why[$name]:-}" ] && printf '%s\t%s\n' "$name" "${why[$name]}"
    done
    return 0
}

# Each workspace package's directory relative to the root, for rule 1.
head_dirs=$(cargo metadata --format-version 1 --no-deps --offline |
    jq -c --arg root "$root/" '[.packages[] | {key: .name,
        value: {dir: (.manifest_path | ltrimstr($root) | rtrimstr("Cargo.toml") | rtrimstr("/"))}}] | from_entries')
order=$(publish_order "$head_meta")
internal=$(grep -vx "$root_name" <<<"$order" || true)

version_of() { jq -r --arg n "$1" '.[$n].version // empty' <<<"$2"; }

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

# Whether name@version is on crates.io. Any answer but "here" or "no such
# crate" is an error, so that a network failure cannot pass for "unpublished"
# and send a crate to be published twice, or for "published" and skip one.
on_crates_io() {
    local body status
    body=$(mktemp)
    status=$(curl -sS -o "$body" -w '%{http_code}' "https://index.crates.io/$(index_path "$1")") ||
        { rm -f "$body"; die "could not reach the crates.io index for $1"; }
    case $status in
        200) jq -e --arg v "$2" 'select(.vers == $v)' "$body" >/dev/null && { rm -f "$body"; return 0; } ;;
        404) ;;
        *) rm -f "$body"; die "the crates.io index answered $status for $1" ;;
    esac
    rm -f "$body"
    return 1
}

cmd=${1:-check}
case $cmd in
    unpublished)
        for name in $order; do
            v=$(version_of "$name" "$head_meta")
            if on_crates_io "$name" "$v"; then
                echo "::notice::$name@$v is on crates.io already; skipping it" >&2
            else
                echo "$name"
            fi
        done
        ;;

    needs-bump)
        base=$(last_tag)
        [ -n "$base" ] || die "no v* tag to compare against"
        needs_bump "$base" "$(meta_at "$base")"
        ;;

    check)
        base=$(last_tag)
        if [ -z "$base" ]; then
            echo "::notice::no earlier v* tag; nothing to check a release against"
            exit 0
        fi
        base_meta=$(meta_at "$base")
        root_now=$(version_of "$root_name" "$head_meta")
        root_then=$(version_of "$root_name" "$base_meta")
        if [ "$root_now" = "$root_then" ]; then
            echo "::notice::not a release ($root_name is still $root_now, as in $base); crate versions are checked when it is bumped"
            exit 0
        fi

        declare -A why=()
        while IFS=$'\t' read -r name reason; do
            [ -n "$name" ] && why[$name]=$reason
        done < <(needs_bump "$base" "$base_meta")

        failed=0
        publish=()
        for name in $internal; do
            now=$(version_of "$name" "$head_meta")
            was=$(version_of "$name" "$base_meta")
            reason=${why[$name]:-}
            # Rule 3 over the versions as they are: a dependency bumped without
            # cause still moves this crate's pin.
            if [ -z "$reason" ]; then
                for dep in $(jq -r --arg n "$name" '.[$n].internal[]' <<<"$head_meta"); do
                    if [ "$(version_of "$dep" "$head_meta")" != "$(version_of "$dep" "$base_meta")" ]; then
                        reason="it depends on $dep, which is bumped"
                        break
                    fi
                done
            fi
            dir=$(jq -r --arg n "$name" '.[$n].dir' <<<"$head_dirs")
            if [ -n "$reason" ]; then
                if [ "$now" = "$was" ]; then
                    echo "::error::$name changed since $base ($reason) but is still $now; bump it and its = pin"
                    failed=1
                elif [ -n "$was" ] && [ "$(printf '%s\n%s\n' "$was" "$now" | sort -V | tail -n1)" != "$now" ]; then
                    echo "::error::$name went from $was to $now; a release only moves a version up"
                    failed=1
                else
                    publish+=("$name")
                fi
            elif [ "$now" != "$was" ]; then
                echo "::error::$name is bumped to $now but nothing in it changed since $base ($dir/, its manifest and its internal dependencies are as they were); put it back to $was"
                failed=1
            fi
        done
        [ "$failed" = 0 ] || exit 1
        publish+=("$root_name")
        echo "::notice::release $root_now (since $base) publishes: ${publish[*]}"
        ;;

    *)
        echo "usage: $0 [check|needs-bump|unpublished]" >&2
        exit 2
        ;;
esac
