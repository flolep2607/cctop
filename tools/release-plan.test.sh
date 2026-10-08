#!/usr/bin/env bash
# Exercises tools/release-plan.sh against throwaway releases in a scratch git
# repository built from this checkout — never against this repository's own
# tags. Each case starts from a commit tagged with the current versions, makes
# one release-shaped commit on top, and checks what the guard says about it.
#
#   tools/release-plan.test.sh
#
# Needs git, cargo, jq; no network (the crates.io lookup is not exercised) and
# no cargo-semver-checks: CCTOP_BREAKING says which crates' API broke, the
# question the tool answers for real on a release.

set -euo pipefail

src=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Enough of the tree for `cargo metadata` to see every package as it is:
# the manifests, the sources cargo discovers targets from, and what the
# manifests point at. The mirrored docs and the web app are neither.
git -C "$src" archive HEAD -- . ':(exclude)docs' ':(exclude)RnD' ':(exclude)web' | tar -x -C "$work"
# The script under test as it is in the working tree, edits included.
cp "$src/tools/release-plan.sh" "$src/tools/bump.sh" "$work/tools/"
cp "$src/rust-toolchain.toml" "$work/"
cd "$work"
git init -q -b main
git config user.name test
git config user.email test@example.invalid
git add -A
git commit -q -m base

v() { cargo metadata --format-version 1 --no-deps --offline | jq -r --arg n "$1" '.packages[] | select(.name == $n) | .version'; }
base=$(v cctop)
git tag "v$base"
next=$(awk -F. '{print $1 "." $2 "." $3 + 1}' <<<"$base")
# No API broke unless a case says so.
export CCTOP_BREAKING=

crate_version() { sed -n '0,/^version = "/s/^version = "\([^"]*\)".*/\1/p' "crates/$1/Cargo.toml"; }
set_crate_version() { sed -i "0,/^version = \".*\"/s//version = \"$2\"/" "crates/$1/Cargo.toml"; }
# Moves an internal crate (tunnel, core, serve, ui) up one patch; its
# requirement stays, as a non-breaking bump leaves it.
bump() { set_crate_version "$1" "$(crate_version "$1" | awk -F. '{print $1 "." $2 "." $3 + 1}')"; }
# Moves an internal crate up one 0.x minor, the breaking step, without its
# requirement.
bump_breaking() { set_crate_version "$1" "$(crate_version "$1" | awk -F. '{print $1 "." $2 + 1 ".0"}')"; }
# Moves its requirement under [workspace.dependencies] to its version.
follow() {
    local req
    req=$(crate_version "$1" | cut -d. -f1-2)
    sed -i "s/^\(cctop-$1 = .*version = \"\)[^\"]*\"/\1$req\"/" Cargo.toml
}
# What cargo-semver-checks would say of a crate whose API broke.
breaks() { CCTOP_BREAKING="$CCTOP_BREAKING cctop-$1"; }
# Moves the root, which is what makes a commit a release.
bump_root() { sed -i "0,/^version = \".*\"/s//version = \"$next\"/" Cargo.toml; }
touch_crate() { echo "// changed" >>"crates/$1/src/lib.rs"; }

pass=0
fail=0

# case <name> <expected exit: 0|1> <pattern the output must match>... -- <edits>
case_() {
    local name=$1 want=$2
    shift 2
    local patterns=()
    while [ "$1" != -- ]; do
        patterns+=("$1")
        shift
    done
    shift
    git checkout -q -f "v$base"
    git clean -qfdx
    CCTOP_BREAKING=
    eval "$*"
    git commit -q -am "$name"
    local out rc=0
    out=$(tools/release-plan.sh check 2>&1) || rc=$?
    local ok=1
    [ "$rc" = "$want" ] || ok=0
    for p in "${patterns[@]}"; do
        grep -qE -- "$p" <<<"$out" || ok=0
    done
    if [ "$ok" = 1 ]; then
        pass=$((pass + 1))
        echo "ok   $name"
    else
        fail=$((fail + 1))
        echo "FAIL $name (exit $rc, wanted $want)"
    fi
    sed 's/^/       /' <<<"$out"
}

case_ "(a) ui changed, ui and root bumped" 0 "publishes: cctop-ui cctop$" -- \
    touch_crate ui ';' bump ui ';' bump_root
case_ "(b) ui changed, only root bumped" 1 "::error::cctop-ui changed .*crates/ui/.* still [0-9]+\.[0-9]+\.[0-9]+" -- \
    touch_crate ui ';' bump_root
case_ "(c) core changed, API kept, core and root bumped" 0 "publishes: cctop-core cctop$" -- \
    touch_crate core ';' bump core ';' bump_root
case_ "(c2) tunnel changed, API kept, tunnel and root bumped" 0 "publishes: cctop-tunnel cctop$" -- \
    touch_crate tunnel ';' bump tunnel ';' bump_root
case_ "(c3) core changed, API kept, serve and ui bumped too" 1 \
    "::error::cctop-serve is bumped .* nothing in it changed" "::error::cctop-ui is bumped .* nothing in it changed" -- \
    touch_crate core ';' bump core ';' bump serve ';' bump ui ';' bump_root
case_ "(d) core API broke, core given a patch" 1 \
    "::error::cctop-core broke its public API .* make it [0-9]+\.[0-9]+\.0 and move its requirement" -- \
    touch_crate core ';' breaks core ';' bump core ';' bump_root
case_ "(d2) core API broke, core given a breaking bump, requirement left behind" 1 \
    "::error::cctop-ui requires cctop-core \^0\.[0-9]+, and cctop-core is 0\.[0-9]+\.0" \
    "::error::cctop requires cctop-core" -- \
    touch_crate core ';' breaks core ';' bump_breaking core ';' bump_root
case_ "(d3) core API broke, core's breaking bump and requirement, dependents not bumped" 1 \
    "::error::cctop-serve changed .*requirement on cctop-core moved" \
    "::error::cctop-ui changed .*requirement on cctop-core moved" -- \
    touch_crate core ';' breaks core ';' bump_breaking core ';' follow core ';' bump_root
case_ "(d4) core API broke, core's breaking bump, dependents bumped" 0 \
    "publishes: cctop-core cctop-serve cctop-ui cctop$" -- \
    touch_crate core ';' breaks core ';' bump_breaking core ';' follow core ';' \
    bump serve ';' bump ui ';' bump_root
case_ "(d5) tunnel API broke: core follows with a patch, serve and ui stay" 0 \
    "publishes: cctop-tunnel cctop-core cctop$" -- \
    touch_crate tunnel ';' breaks tunnel ';' bump_breaking tunnel ';' follow tunnel ';' \
    bump core ';' bump_root
case_ "(e) ratatui bumped in the workspace, root bumped" 1 \
    "::error::cctop-ui changed .*packaged manifest" "::error::cctop-core changed .*packaged manifest" -- \
    "sed -i 's/^ratatui = \"[^\"]*\"/ratatui = \"0.99.0\"/' Cargo.toml" ';' bump_root
case_ "(e2) ratatui-image (ui's alone) bumped, ui and root bumped" 0 "publishes: cctop-ui cctop$" -- \
    "sed -i 's/^ratatui-image = .*/ratatui-image = \"99.0.0\"/' Cargo.toml" ';' bump ui ';' bump_root
case_ "(f) ui changed, nothing bumped" 0 "not a release" -- \
    touch_crate ui
case_ "(g) serve bumped though nothing in it changed" 1 "::error::cctop-serve is bumped .* nothing in it changed" -- \
    bump serve ';' bump_root
case_ "(h) only the binary changed, root bumped" 0 "publishes: cctop$" -- \
    "echo '// changed' >>src/main.rs" ';' bump_root
case_ "(i) a [workspace.package] field changed, root bumped" 1 \
    "::error::cctop-core changed .*packaged manifest" "::error::cctop-ui changed" "::error::cctop-serve changed" -- \
    "sed -i 's/^rust-version = \".*\"/rust-version = \"1.99\"/' Cargo.toml" ';' bump_root

# A workflow_dispatch re-run checks out the release's own tag, where the root
# matches the newest tag; the guard must look past it to the one before.
git checkout -q -f "v$base"
touch_crate ui
bump ui
bump_root
git commit -q -am rerun
git tag "v$next"
out=$(tools/release-plan.sh check 2>&1) || true
if grep -q "publishes: cctop-ui cctop$" <<<"$out"; then
    pass=$((pass + 1))
    echo "ok   (j) re-run on the release's own tag"
else
    fail=$((fail + 1))
    echo "FAIL (j) re-run on the release's own tag"
fi
sed 's/^/       /' <<<"$out"
git tag -d "v$next" >/dev/null

# needs-bump closes rule 3 before anything is bumped, which is what bump.sh
# relies on: a core change alone moves core, and one that breaks its API
# moves its dependents too.
needs() {
    local name=$1 want=$2 got
    got=$(CCTOP_BREAKING=$3 tools/release-plan.sh needs-bump | cut -f1,3 | tr '\t\n' ': ')
    if [ "$got" = "$want" ]; then
        pass=$((pass + 1))
        echo "ok   needs-bump $name: $got"
    else
        fail=$((fail + 1))
        echo "FAIL needs-bump $name: $got (wanted $want)"
    fi
}
git checkout -q -f "v$base"
touch_crate core
git commit -q -am core
needs "after a core change" "cctop-core:patch " ""
needs "after a core API break" "cctop-core:breaking cctop-serve:patch cctop-ui:patch " cctop-core
# Asked about a crate that did not change, the tool's answer is moot: an
# unchanged crate is never sent to it.
needs "with an unchanged crate named as broken" "cctop-core:patch " cctop-ui

# bump.sh, after a serve change: serve and the root move, nothing else, and
# the guard agrees.
git checkout -q -f "v$base"
touch_crate serve
git commit -q -am serve
CCTOP_BUMP_SKIP_LOCK=1 tools/bump.sh "$next" | sed 's/^/       /'
git commit -q -am bumped
out=$(tools/release-plan.sh check 2>&1) || true
if grep -q "publishes: cctop-serve cctop$" <<<"$out"; then
    pass=$((pass + 1))
    echo "ok   bump.sh after a serve change"
else
    fail=$((fail + 1))
    echo "FAIL bump.sh after a serve change"
fi
sed 's/^/       /' <<<"$out"

# bump.sh, after a core API break: core takes 0.x+1.0 and its requirement
# follows, serve and ui take a patch for the moved requirement.
git checkout -q -f "v$base"
touch_crate core
git commit -q -am core
CCTOP_BREAKING=cctop-core CCTOP_BUMP_SKIP_LOCK=1 tools/bump.sh "$next" | sed 's/^/       /'
git commit -q -am bumped
out=$(CCTOP_BREAKING=cctop-core tools/release-plan.sh check 2>&1) || true
if grep -q "publishes: cctop-core cctop-serve cctop-ui cctop$" <<<"$out"; then
    pass=$((pass + 1))
    echo "ok   bump.sh after a core API break"
else
    fail=$((fail + 1))
    echo "FAIL bump.sh after a core API break"
fi
sed 's/^/       /' <<<"$out"

# Without CCTOP_BREAKING the guard asks cargo-semver-checks, and one of
# another version than rust-toolchain.toml pins is refused rather than
# trusted. A fake on PATH stands in for whatever is installed.
git checkout -q -f "v$base"
touch_crate core
bump core
bump_root
git commit -q -am release
mkdir -p "$work/fakebin"
printf '#!/bin/sh\n[ "$1" = semver-checks ] && [ "$2" = --version ] && { echo "cargo-semver-checks 0.0.1"; exit 0; }\nexec %s "$@"\n' \
    "$(command -v cargo)" >"$work/fakebin/cargo"
chmod +x "$work/fakebin/cargo"
rc=0
out=$(env -u CCTOP_BREAKING PATH="$work/fakebin:$PATH" tools/release-plan.sh check 2>&1) || rc=$?
if [ "$rc" = 1 ] && grep -q "cargo-semver-checks is 0.0.1, and rust-toolchain.toml pins" <<<"$out"; then
    pass=$((pass + 1))
    echo "ok   an unpinned cargo-semver-checks is refused"
else
    fail=$((fail + 1))
    echo "FAIL an unpinned cargo-semver-checks is refused (exit $rc)"
fi
sed 's/^/       /' <<<"$out"

echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
