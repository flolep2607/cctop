#!/usr/bin/env bash
# Exercises tools/release-plan.sh against throwaway releases in a scratch git
# repository built from this checkout — never against this repository's own
# tags. Each case starts from a commit tagged with the current versions, makes
# one release-shaped commit on top, and checks what the guard says about it.
#
#   tools/release-plan.test.sh
#
# Needs git, cargo, jq; no network (the crates.io lookup is not exercised).

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

# Moves an internal crate (core, serve, ui) to $next, with its `=` pin.
bump() {
    sed -i "0,/^version = \".*\"/s//version = \"$next\"/" "crates/$1/Cargo.toml"
    sed -i "s/^\(cctop-$1 = .*version = \"=\)[^\"]*\"/\1$next\"/" Cargo.toml
}
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
case_ "(b) ui changed, only root bumped" 1 "::error::cctop-ui changed .*crates/ui/.* still $base" -- \
    touch_crate ui ';' bump_root
case_ "(c) core changed, all four bumped" 0 "publishes: cctop-core cctop-serve cctop-ui cctop$" -- \
    touch_crate core ';' bump core ';' bump serve ';' bump ui ';' bump_root
case_ "(d) core changed, only core and root bumped" 1 \
    "::error::cctop-serve changed .*depends on cctop-core" "::error::cctop-ui changed .*depends on cctop-core" -- \
    touch_crate core ';' bump core ';' bump_root
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
# relies on.
git checkout -q -f "v$base"
touch_crate core
git commit -q -am core
got=$(tools/release-plan.sh needs-bump | cut -f1 | tr '\n' ' ')
if [ "$got" = "cctop-core cctop-serve cctop-ui " ]; then
    pass=$((pass + 1))
    echo "ok   needs-bump after a core change: $got"
else
    fail=$((fail + 1))
    echo "FAIL needs-bump after a core change: $got"
fi

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

echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
