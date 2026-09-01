#!/usr/bin/env bash
# Mirror rmux's own documentation into docs/rmux/.
#
# cctop hands every tab's agent to rmux and drives it by its command surface —
# `new-session -A`, `list-panes -F`, `set-option`, `web-share`. None of that is
# a stable interface anybody promised us, and the first cctop would hear of a
# change is a pane that comes back empty. This pulls rmux's published docs down
# so a change can be read against a fixed copy instead of a live website.
#
# It is a mirror, not a fork: everything under docs/rmux/ except this script and
# SOURCE.md is upstream's, and is overwritten wholesale on the next run.
#
#   ./docs/rmux/pull.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

command -v git >/dev/null || { echo "missing: git" >&2; exit 1; }

repo="https://github.com/Helvesec/rmux"
git clone --depth 1 --quiet "$repo" "$tmp/rmux"
version="$(git -C "$tmp/rmux" rev-parse --short HEAD)"

# Everything upstream keeps as prose, and nothing it keeps as pictures: the
# repo's docs directory is two thirds sidebar artwork by weight and says nothing
# about a command surface. The translations go for the same reason they go in
# docs/harnesses — three more copies of one README.
find "$root" -mindepth 1 -maxdepth 1 \
  ! -name pull.sh ! -name SOURCE.md -exec rm -rf {} +
mkdir -p "$root/docs"
cp "$tmp/rmux/README.md" "$tmp/rmux/CHANGELOG.md" "$root/"
(cd "$tmp/rmux/docs" && find . -name '*.md' ! -path './i18n/*' -print0 \
  | tar --null -cf - -T -) | (cd "$root/docs" && tar -xf -)
mkdir -p "$root/docs/man"
cp "$tmp/rmux/docs/man/rmux.1" "$root/docs/man/"

cat > "$root/SOURCE.md" <<EOF
# Source

Mirrored from <$repo> (\`README.md\`, \`CHANGELOG.md\` and \`docs/\`) at
\`$version\` on $(date -u +%Y-%m-%d) by \`docs/rmux/pull.sh\`.

rmux is the multiplexer cctop hands every tab's agent to, and whose
\`web-share\` puts an agent's terminal in a browser. cctop meets it as a command
surface — see \`src/rmux.rs\`.

Two things this drops. The artwork: \`docs/\` is mostly SVG sidebar and wordmark
files, which outweigh the prose and say nothing about a command. And the
translations under \`docs/i18n/\`, which are the README again in three more
languages.

One thing it cannot take. <https://rmux.io/docs/> is the fuller documentation —
get-started, CLI, API, examples — and it is an interactive site that serves its
prose only as rendered HTML around a playground, with no markdown behind a page.
\`docs/man/rmux.1\` is the CLI reference that is written down, and \`rmux
<command> --help\` is the one that ships with the binary.

Upstream's own licence applies to everything in this directory.
EOF
echo "mirrored rmux docs at $version"
