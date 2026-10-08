#!/usr/bin/env bash
# Run only the tests that could plausibly be affected by what changed.
#
# Why this exists: `cargo test --workspace` in cctop spends most of its wall
# time waiting on sleeps, ptys and subprocesses, and it goes *flaky* when
# several of them run at once on this 6-core box — two wall-clock tests failed
# across three consecutive full runs while six lanes were compiling. A red
# full-suite run under load is a false signal.
#
# Usage:
#   ./targeted-test.sh                 # diff against the merge-base of main
#   ./targeted-test.sh crates/ui/src/filter.rs crates/ui/src/ansi.rs
#   ./targeted-test.sh --all           # force the full suite (one lane, alone)
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"
# `-D warnings` comes from .cargo/config.toml, as for every cargo command here.

if [[ "${1:-}" == "--all" ]]; then
    exec cargo test --workspace --all-targets
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

# Map a source file to the package it belongs to and the test filter that
# covers it. A test's path is its file's module path inside its own crate, so
# `crates/ui/src/filter.rs` is `-p cctop-ui` with `filter::`, and
# `src/cli.rs` is the binary, `-p cctop`, with `cli::`.
#
# Only the crate the file is in is tested, not the crates built on it: a change
# to core that breaks the UI's tests is what the final full run is for.
packages=()
filters=()
whole=false        # some package's whole suite, with no filter
everything=false   # every package's whole suite
integration=false  # the binary's own tests/, which spawn it

module_of() {      # crate-relative path under src/ -> module filter
    local m=${1%.rs}
    m=${m%/mod}
    [[ $m == lib || $m == main ]] && return
    echo "${m//\//::}::"
}

for f in "${files[@]}"; do
    case "$f" in
        crates/core/build.rs) everything=true ;;
        crates/*/src/*.rs)
            rest=${f#crates/}
            krate=${rest%%/*}
            packages+=("cctop-$krate")
            module=$(module_of "${rest#*/src/}")
            if [[ -n $module ]]; then filters+=("$module"); else whole=true; fi
            ;;
        src/*.rs)
            packages+=(cctop)
            module=$(module_of "${f#src/}")
            if [[ -n $module ]]; then filters+=("$module"); else whole=true; fi
            ;;
        tests/*.rs)
            packages+=(cctop)
            integration=true
            ;;
        *) everything=true ;;
    esac
    # `cctop hook` is what the binary's integration tests spawn and judge.
    case "$f" in
        crates/core/src/hook.rs|crates/core/src/hook/*) packages+=(cctop); integration=true ;;
    esac
done

if $everything; then
    echo "changes reach every crate; running the full suite"
    echo "  (run this alone — it is timing-sensitive under load)"
    exec cargo test --workspace --all-targets
fi
if $whole; then
    filters=()
fi

# De-duplicate; an empty filter list means "these packages' whole suites".
mapfile -t packages < <(printf '%s\n' "${packages[@]}" | sort -u)
mapfile -t filters < <(printf '%s\n' "${filters[@]}" | sed '/^$/d' | sort -u)

echo "changed:"
printf '  %s\n' "${files[@]}"
echo
echo "in: ${packages[*]}"
if [[ ${#filters[@]} -gt 0 ]]; then
    echo "running $((${#filters[@]})) scoped test filter(s):"
    printf '  %s\n' "${filters[@]}"
else
    echo "running their whole suites"
fi
echo

# One cargo invocation for every package and filter: libtest takes several
# filters and runs a test matching any of them, so everything is built and each
# harness started once.
#
# And only the unit-test harnesses, unless the change reaches the binary's own
# integration tests. Those spawn a plain build of the binary, which is a whole
# extra link competing for the same cores to run two files that only `cctop
# hook` and the argument parsing can break.
args=()
for p in "${packages[@]}"; do args+=(-p "$p"); done
targets=(--lib --bins)
if $integration; then
    targets=(--lib --bins --tests)
fi
out=$(cargo test "${args[@]}" "${targets[@]}" -- "${filters[@]}" 2>&1)
status=$?
printf '%s\n' "$out" | grep -E '^test result|^error|FAILED|panicked at' || true
if [[ $status -ne 0 ]]; then
    # Don't truncate the failure: that is the whole point of running it.
    printf '%s\n' "$out" | grep -E -A15 '^failures:' | head -40
fi
exit $status
