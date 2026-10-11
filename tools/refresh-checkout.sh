#!/usr/bin/env bash
# Brings the files beside a bare repository up to origin/main.
#
# The main clone here is bare (core.bare = true) so that nobody edits it while
# agents work in worktrees, but the folder still holds a copy of the files, and
# a session started there reads its CLAUDE.md, agents and skills. Git never
# updates that copy, so it drifts: a session once loaded agents that main had
# deleted forty-nine commits earlier.
#
#   tools/refresh-checkout.sh [--from <commit>]
#
# The copy is overwritten only when it is exactly the commit it was last
# refreshed to (kept in the git dir as `checkout-snapshot`), so an edit made
# there by hand is reported and left alone rather than lost. `--from` names
# that commit the first time, before anything has been recorded. Files main
# deleted since are deleted; untracked files are never touched.
#
# Run from the reference-transaction hook whenever a fetch moves origin/main;
# see CONTRIBUTING.md.
set -euo pipefail

common=$(git rev-parse --path-format=absolute --git-common-dir)
[ "$(git --git-dir="$common" config --bool core.bare)" = true ] || {
    echo "not a bare clone with files beside it; nothing to refresh" >&2
    exit 0
}
root=$(dirname "$common")
record="$common/checkout-snapshot"
g() { git --git-dir="$common" --work-tree="$root" "$@"; }

from=$(cat "$record" 2>/dev/null || true)
[ "${1:-}" = --from ] && from=$(g rev-parse --verify "$2^{commit}")
[ -n "$from" ] || {
    echo "no snapshot recorded yet: name the commit the files are at with --from" >&2
    exit 1
}
to=$(g rev-parse --verify origin/main^{commit})
[ "$from" = "$to" ] && exit 0

# The paths the snapshot tracks, compared byte for byte with what is there.
changed=$(g diff --name-only "$from" -- . || true)
if [ -n "$changed" ]; then
    echo "$root differs from $from in:" >&2
    echo "$changed" | sed 's/^/  /' >&2
    echo "not refreshing; move those edits to a worktree, then run this again" >&2
    exit 1
fi

gone=$(g diff --name-only --diff-filter=D "$from" "$to")
if [ -n "$gone" ]; then
    # And the folders that leaves empty: a skill directory with no SKILL.md is
    # still a directory someone has to wonder about.
    (cd "$root" && echo "$gone" | xargs -d '\n' rm -f -- &&
        echo "$gone" | xargs -d '\n' dirname | sort -u | xargs -d '\n' rmdir -p --ignore-fail-on-non-empty -- 2>/dev/null || true)
fi
g checkout -f "$to" -- .
echo "$to" >"$record"
echo "refreshed $root: ${from:0:7} -> ${to:0:7}"
