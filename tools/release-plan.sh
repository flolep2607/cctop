#!/usr/bin/env bash
# Which crates a release has to bump, and which it has to publish.
#
# cctop is five crates — cctop-tunnel, cctop-core, cctop-serve and cctop-ui
# under crates/, and the binary at the root — and each has its own version. The root's is
# the release version and the tag, and every release bumps it. An internal
# crate is bumped only when it changed since the last release tag, where
# "changed" is any of:
#
#   1. a file under its directory changed;
#   2. its packaged manifest changed without one — a `[workspace.dependencies]`
#      entry it uses was bumped, its requirement on an internal crate moved, or
#      a `[workspace.package]` field it inherits changed. Compared as `cargo
#      metadata` sees the package at the tag and at HEAD, so that what counts
#      is what cargo would publish, not how the TOML is laid out;
#   3. an internal crate it depends on takes a breaking bump. The internal
#      requirements are carets on the compatible part of the version ("0.28"),
#      so a non-breaking bump of core leaves a published cctop-ui@0.28.3
#      building beside it, and nothing else moves. A breaking one (0.28.x ->
#      0.29.0) has to move the requirement, and a moved requirement is rule 2;
#      this rule is the same thing said before the bump, so that `needs-bump`
#      can answer it.
#
# Whether a crate's public API broke is cargo-semver-checks' answer, comparing
# the crate at HEAD with the same crate at the tag. Only crates that changed
# under rules 1-2 are asked, and only on a release, because each one builds the
# crate's rustdoc twice. The version is pinned in rust-toolchain.toml, beside
# the toolchain whose rustdoc it reads. CCTOP_BREAKING, set (even empty), stands
# in for it: the space-separated crates whose API broke. The tests use it, and
# so can a human without the tool who knows the answer.
#
# Commands:
#
#   check        The guard `verify` runs. On a release commit (the root version
#                differs from the last tag's) it fails, one `::error::` line per
#                crate, when a changed crate kept its version, an unchanged one
#                was bumped, a crate whose API broke took less than a breaking
#                bump, or an internal requirement is not the caret on its
#                dependency's current version; and prints what the release
#                will publish. On
#                any other commit it says so and passes: a feature branch
#                changes crates without bumping them, and that is fine.
#   needs-bump   The internal crates rules 1-3 say must be bumped, as
#                "name<TAB>reason<TAB>patch|breaking", closing rule 3 over the
#                API breaks rather than over the versions, so it answers before
#                anything is bumped. `tools/bump.sh` reads it.
#   base         The tag the other commands compare against.
#   unpublished  The workspace crates whose current version is not on
#                crates.io yet, in the order they must be published. What
#                `publish-crate` publishes.
#
# It never runs `cargo publish` in any form. It needs git, cargo, jq and curl,
# a checkout with its tags and history (`fetch-depth: 0` in CI), and for
# `check` on a release and `needs-bump`, cargo-semver-checks or CCTOP_BREAKING.
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
# Derived rather than listed, so a new crate needs no edit here: cctop-tunnel,
# the fifth, comes out first because core depends on it.
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
    local version tag
    version=$(jq -r --arg r "$root_name" '.[$r].version' <<<"$head_meta")
    tag=$(git describe --tags --abbrev=0 --match 'v*' HEAD 2>/dev/null) || return 0
    # A workflow_dispatch re-run of a release checks out the tag itself, and
    # comparing a release with itself would call nothing changed, so there it
    # is the tag before. Anywhere else, a newest tag that matches the root is
    # what says this commit is not a release.
    if [ "$tag" = "v$version" ] && [ "$(git rev-parse "$tag^{commit}")" = "$(git rev-parse HEAD)" ]; then
        tag=$(git describe --tags --abbrev=0 --match 'v*' --exclude "$tag" HEAD 2>/dev/null) || return 0
    fi
    echo "$tag"
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
    # The crate's own version line is the one edit that is not a change: it
    # is the bump these rules ask for.
    if ! git diff --quiet "$base" HEAD -- "$dir/" ":(exclude)$dir/Cargo.toml" ||
        [ "$(git show "$base:$dir/Cargo.toml" | sed '0,/^version = /{/^version = /d}')" != \
            "$(git show "HEAD:$dir/Cargo.toml" | sed '0,/^version = /{/^version = /d}')" ]; then
        echo "$dir/"
        return
    fi
    # A moved requirement on an internal crate is a real manifest change —
    # the published crate asks for something else — but it has one cause
    # worth naming, so it is told apart from the rest.
    local shape='.[$n] | del(.version) | .pins |= map(del(.req))'
    if [ "$(jq -c --arg n "$name" "$shape" <<<"$head_meta")" != \
        "$(jq -c --arg n "$name" "$shape" <<<"$base_meta")" ]; then
        echo "its packaged manifest"
        return
    fi
    local moved
    moved=$(jq -r --arg n "$name" --argjson base "$base_meta" '
        .[$n].pins[] as $p
        | ($base[$n].pins[] | select(.name == $p.name and (.kind // "") == ($p.kind // "")) | .req) as $was
        | select($was != $p.req) | "\($p.name) moved from \($was) to \($p.req)"' <<<"$head_meta" |
        sort -u | paste -sd, - | sed 's/,/, /g')
    if [ -n "$moved" ]; then echo "its requirement on $moved"; fi
}

# The caret a dependent should hold on a crate at this version: what cargo
# calls compatible, so that it moves exactly when the version breaks.
compat() {
    local major minor
    IFS=. read -r major minor _ <<<"$1"
    if [ "$major" = 0 ]; then echo "0.$minor"; else echo "$major"; fi
}

# Whether going from $1 to $2 is a breaking bump in cargo's sense.
is_breaking() { [ "$(compat "$1")" != "$(compat "$2")" ]; }

# The pinned cargo-semver-checks, from rust-toolchain.toml.
semver_checks_pin() { sed -n 's/^# cargo-semver-checks \([0-9][0-9.]*\)$/\1/p' rust-toolchain.toml; }

# Which of the crates named in $3.. broke their public API since $1, one per
# line. A crate new since the tag has no API to break and is not asked.
breaking() {
    local base=$1 base_meta=$2 name
    shift 2
    if [ -n "${CCTOP_BREAKING+set}" ]; then
        for name in $CCTOP_BREAKING; do
            grep -qx -- "$name" <<<"$internal" || die "CCTOP_BREAKING names $name, which is not an internal crate"
        done
        for name in "$@"; do
            if grep -qx -- "$name" <<<"$(tr ' ' '\n' <<<"$CCTOP_BREAKING")"; then echo "$name"; fi
        done
        return 0
    fi
    [ $# -gt 0 ] || return 0
    local pin have
    pin=$(semver_checks_pin)
    [ -n "$pin" ] || die "rust-toolchain.toml names no cargo-semver-checks version"
    have=$(cargo semver-checks --version 2>/dev/null | awk '{print $2}') ||
        die "cargo-semver-checks is not installed: cargo install cargo-semver-checks@$pin --locked, or set CCTOP_BREAKING to the crates whose API broke"
    [ "$have" = "$pin" ] ||
        die "cargo-semver-checks is $have, and rust-toolchain.toml pins $pin: it has to read this toolchain's rustdoc"
    local out rc
    for name in "$@"; do
        jq -e --arg n "$name" 'has($n)' <<<"$base_meta" >/dev/null || continue
        # A crate that already took a breaking bump is counted as broken
        # without asking: no verdict could require more of it, and its
        # dependents owe the same bumps either way. It is also the way out when
        # the baseline no longer builds: cctop-core 0.31.0 requires
        # cctop-rmux-client 0.10, which cannot compile on the rustix 1.1.5 the
        # tool resolves, and the conservative answer then is the breaking
        # bump, not a hand-written "nothing broke".
        if is_breaking "$(version_of "$name" "$base_meta")" "$(version_of "$name" "$head_meta")"; then
            echo "$name"
            continue
        fi
        echo "cargo-semver-checks: $name against $base" >&2
        # `--release-type minor` asks only "is anything breaking?": under it
        # just the lints that need a breaking bump fail, whichever way the
        # tool reads minor on a 0.x crate, and the versions themselves are
        # left out of the question, so it answers the same before the bump as
        # after.
        rc=0
        out=$(cargo semver-checks check-release --package "$name" --baseline-rev "$base" \
            --release-type minor 2>&1) || rc=$?
        if [ "$rc" = 0 ]; then
            continue
        elif grep -q 'semver requires new' <<<"$out"; then
            sed 's/^/    /' <<<"$out" >&2
            echo "$name"
        else
            # A build failure is not an API verdict: calling it breaking would
            # send a needless 0.x minor to crates.io, which cannot be undone.
            sed 's/^/    /' <<<"$out" >&2
            die "cargo-semver-checks failed on $name (exit $rc) without a verdict"
        fi
    done
}

# Every internal crate rules 1-3 say must be bumped, as
# "name<TAB>reason<TAB>patch|breaking". Rule 3 is closed over the API breaks,
# not over the versions, so the answer does not depend on whether the bumps
# have been made yet. A requirement that moved because the bump was made
# already is rule 2 by then, and gives the same answer.
needs_bump() {
    local base=$1 base_meta=$2 name reason dep
    declare -A why=() level=()
    local changed=()
    for name in $internal; do
        if ! jq -e --arg n "$name" 'has($n)' <<<"$base_meta" >/dev/null; then
            why[$name]="it is new since $base"
            continue
        fi
        reason=$(own_change "$name" "$base" "$base_meta")
        if [ -n "$reason" ]; then
            why[$name]=$reason
            changed+=("$name")
        fi
    done
    local broke
    broke=$(breaking "$base" "$base_meta" "${changed[@]}") || return 1
    for name in $broke; do
        level[$name]=breaking
        why[$name]="${why[$name]}; its public API broke"
    done
    # In publishing order, so a dependency's verdict is settled before its
    # dependents look at it. Only a breaking bump reaches a dependent: it is
    # the one that moves the requirement.
    for name in $internal; do
        [ -n "${why[$name]:-}" ] && continue
        for dep in $(jq -r --arg n "$name" '.[$n].internal[]' <<<"$head_meta"); do
            if [ "${level[$dep]:-}" = breaking ]; then
                why[$name]="its requirement on $dep has to move: $dep takes a breaking bump"
                break
            fi
        done
    done
    for name in $internal; do
        [ -n "${why[$name]:-}" ] && printf '%s\t%s\t%s\n' "$name" "${why[$name]}" "${level[$name]:-patch}"
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

# The next breaking version after $1: 0.28.6 -> 0.29.0, 1.2.3 -> 2.0.0.
next_breaking() {
    local major minor
    IFS=. read -r major minor _ <<<"$1"
    if [ "$major" = 0 ]; then echo "0.$((minor + 1)).0"; else echo "$((major + 1)).0.0"; fi
}

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

    base)
        base=$(last_tag)
        [ -n "$base" ] || die "no v* tag to compare against"
        echo "$base"
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

        declare -A why=() level=()
        plan=$(needs_bump "$base" "$base_meta") || exit 1
        while IFS=$'\t' read -r name reason lvl; do
            [ -n "$name" ] || continue
            why[$name]=$reason
            level[$name]=$lvl
        done <<<"$plan"

        failed=0
        publish=()
        for name in $internal; do
            now=$(version_of "$name" "$head_meta")
            was=$(version_of "$name" "$base_meta")
            reason=${why[$name]:-}
            dir=$(jq -r --arg n "$name" '.[$n].dir' <<<"$head_dirs")
            if [ -n "$reason" ]; then
                if [ "$now" = "$was" ]; then
                    if [ "${level[$name]}" = breaking ]; then
                        echo "::error::$name changed since $base ($reason) but is still $now; give it a breaking bump and move its requirement to \"$(compat "$(next_breaking "$now")")\""
                    else
                        echo "::error::$name changed since $base ($reason) but is still $now; bump it"
                    fi
                    failed=1
                elif [ -n "$was" ] && [ "$(printf '%s\n%s\n' "$was" "$now" | sort -V | tail -n1)" != "$now" ]; then
                    echo "::error::$name went from $was to $now; a release only moves a version up"
                    failed=1
                elif [ "${level[$name]}" = breaking ] && [ -n "$was" ] && ! is_breaking "$was" "$now"; then
                    echo "::error::$name broke its public API since $base (cargo-semver-checks) but went only from $was to $now; make it $(next_breaking "$was") and move its requirement to \"$(compat "$(next_breaking "$was")")\""
                    failed=1
                else
                    publish+=("$name")
                fi
            elif [ "$now" != "$was" ]; then
                echo "::error::$name is bumped to $now but nothing in it changed since $base ($dir/, its manifest and its internal dependencies are as they were); put it back to $was"
                failed=1
            fi
        done
        # Every requirement is the caret on its dependency's version as it is,
        # so that the workspace resolves one copy of each crate and a
        # requirement moves exactly when a breaking bump does. cargo would
        # refuse a caret the version has left anyway; this says which line.
        while IFS=$'\t' read -r name dep req; do
            [ -n "$name" ] || continue
            want="^$(compat "$(version_of "$dep" "$head_meta")")"
            if [ "$req" != "$want" ]; then
                echo "::error::$name requires $dep $req, and $dep is $(version_of "$dep" "$head_meta"); make its requirement under [workspace.dependencies] \"${want#^}\""
                failed=1
            fi
        done < <(jq -r '.[] | .name as $n | .pins[] | [$n, .name, .req] | @tsv' <<<"$head_meta" | sort -u)
        [ "$failed" = 0 ] || exit 1
        publish+=("$root_name")
        echo "::notice::release $root_now (since $base) publishes: ${publish[*]}"
        ;;

    *)
        echo "usage: $0 [check|needs-bump|base|unpublished]" >&2
        exit 2
        ;;
esac
