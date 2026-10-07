#!/usr/bin/env bash
# Run only the tests that could plausibly be affected by what changed.
#
# Why this exists: `cargo test --all-targets` in cctop is 58s wall for 11s of
# CPU, and it goes *flaky* when several of them run at once on this 6-core box —
# two wall-clock tests failed across three consecutive full runs while six lanes
# were compiling. A red full-suite run under load is a false signal.
#
# Usage:
#   ./targeted-test.sh                 # diff against the merge-base of main
#   ./targeted-test.sh src/ui/filter.rs src/ui/ansi.rs
#   ./targeted-test.sh --all           # force the full suite (one lane, alone)
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"
# `-D warnings` comes from .cargo/config.toml, as for every cargo command here.

if [[ "${1:-}" == "--all" ]]; then
    exec cargo test --all-targets
fi

# Which files are we judging?
if [[ $# -gt 0 ]]; then
    files=("$@")
else
    base=$(git merge-base HEAD main 2>/dev/null || git merge-base HEAD master 2>/dev/null || echo HEAD)
    mapfile -t files < <(git diff --name-only "$base"...HEAD)
    # Nothing committed yet? Fall back to the working tree.
    if [[ ${#files[@]} -eq 0 ]]; then
        mapfile -t files < <(git diff --name-only HEAD; git diff --name-only --cached)
    fi
fi

if [[ ${#files[@]} -eq 0 ]]; then
    echo "no changed files detected; run with --all to force the suite" >&2
    exit 0
fi

# Map a source file to the test filter that covers it. A test's module path is
# its file's module path, so `src/ui/filter.rs` -> `ui::filter::`.
filters=()
whole_crate=false
add() { filters+=("$1"); }

for f in "${files[@]}"; do
    case "$f" in
        # A change to the crate root reaches everything; don't pretend otherwise.
        src/main.rs) whole_crate=true ;;
        # build.rs changes the cache version -> every cached read is affected.
        build.rs)    whole_crate=true ;;
        *.rs)        add "$(echo "${f%.rs}" | sed 's|^src/||; s|/|::|g')" ;;
        *)           whole_crate=true ;;
    esac
done

if $whole_crate; then
    filters=()
fi

# De-duplicate; an empty list means "the whole crate".
mapfile -t filters < <(printf '%s\n' "${filters[@]}" | sed '/^$/d' | sort -u)

if [[ ${#filters[@]} -eq 0 ]]; then
    echo "changes reach the whole crate; running the full suite"
    echo "  (run this alone — it is timing-sensitive under load)"
    exec cargo test --all-targets
fi

echo "changed:"
printf '  %s\n' "${files[@]}"
echo
echo "running $((${#filters[@]})) scoped test filter(s):"
printf '  %s\n' "${filters[@]}"
echo

# One cargo invocation for every filter: libtest takes several and runs a test
# matching any of them, so the crate is built and the harness started once.
#
# And only the unit-test harness, unless the change reaches the binary's own
# integration tests. `--all-targets` also compiles cctop a second time as a
# plain binary for `tests/`, which those tests spawn: a whole extra build of
# the crate, competing for the same cores, to run two files that only
# `cctop hook` and the argument parsing can break.
targets=(--bin cctop)
for f in "${files[@]}"; do
    case "$f" in
        tests/*|src/hook.rs|src/hook/*) targets=(--all-targets) ;;
    esac
done
out=$(cargo test "${targets[@]}" -- "${filters[@]}" 2>&1)
status=$?
printf '%s\n' "$out" | grep -E '^test result|^error|FAILED|panicked at' || true
if [[ $status -ne 0 ]]; then
    # Don't truncate the failure: that is the whole point of running it.
    printf '%s\n' "$out" | grep -E -A15 '^failures:' | head -40
fi
exit $status
