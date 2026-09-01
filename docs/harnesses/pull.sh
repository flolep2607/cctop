#!/usr/bin/env bash
# Mirror each tracked harness's own documentation into docs/harnesses/<name>/.
#
# cctop reads what seven different agents leave on disk, and every one of them
# changes its transcript format, its config keys and its session layout without
# telling us. This pulls their published docs down so a change can be read
# against a fixed copy instead of a live website.
#
# It is a mirror, not a fork: everything under a harness directory is upstream's
# and is overwritten wholesale on the next run. Nothing here is edited by hand.
#
#   ./docs/harnesses/pull.sh            # all of them
#   ./docs/harnesses/pull.sh claude pi  # just these
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

have() { command -v "$1" >/dev/null || { echo "missing: $1" >&2; exit 1; }; }
have curl; have git

# Written into every harness directory so a stale mirror is obvious.
stamp() { # <dir> <source> <note>
  cat > "$1/SOURCE.md" <<EOF
# Source

Mirrored from $2
on $(date -u +%Y-%m-%d) by \`docs/harnesses/pull.sh\`.

$3

Upstream's own licence applies to everything in this directory.
EOF
}

# Fetch a list of .md URLs in parallel, mirroring their path under <dir>.
# Reads "url<TAB>relative/path" on stdin.
fetch_list() { # <dir>
  local dir="$1"
  xargs -P 8 -n 2 bash -c '
    out="'"$dir"'/$1"
    mkdir -p "$(dirname "$out")"
    curl -sSLf --retry 2 -o "$out" "$0" || echo "  ! $0" >&2
  '
}

# Sparse-checkout a few paths out of a repo and copy them in.
from_repo() { # <dir> <repo-url> <ref> <path>...
  local dir="$1" url="$2" ref="$3"; shift 3
  local work="$tmp/$(basename "$dir")"
  git clone --quiet --depth 1 --filter=blob:none --sparse --branch "$ref" "$url" "$work"
  git -C "$work" sparse-checkout set --no-cone "$@" >/dev/null
  local p
  for p in "$@"; do
    [ -e "$work/$p" ] || continue
    if [ -d "$work/$p" ]; then
      mkdir -p "$dir/$(basename "$p")"
      cp -R "$work/$p/." "$dir/$(basename "$p")/"
    else
      cp "$work/$p" "$dir/"
    fi
  done
  prune_binaries "$dir"
}

# Keep the mirror to prose. Upstream repos carry screenshots and theme
# galleries that would outweigh every word of text in this directory, and none
# of them say anything about a transcript format.
prune_binaries() { # <dir>
  find "$1" -type f ! -name '*.md' ! -name '*.mdx' ! -name '*.txt' \
    ! -name '*.json' ! -name '*.toml' ! -name '*.yaml' ! -name '*.yml' -delete
  find "$1" -mindepth 1 -type d -empty -delete
}

reset_dir() { rm -rf "$root/$1"; mkdir -p "$root/$1"; echo "$root/$1"; }

pull_claude() {
  local dir; dir="$(reset_dir claude)"
  curl -sSLf https://code.claude.com/docs/llms.txt \
    | grep -oE 'https://code\.claude\.com/docs/[^)]+\.md' | sort -u \
    | sed -E 's|(https://code\.claude\.com/docs/en/(.*))|\1\t\2|' \
    | fetch_list "$dir"
  stamp "$dir" "<https://code.claude.com/docs> (the \`.md\` twin of each page listed in its \`llms.txt\`)" \
    "Claude Code. cctop reads its JSONL transcripts under \`~/.claude/projects\` and installs itself into its hook settings."
}

pull_cursor() {
  local dir; dir="$(reset_dir cursor)"
  curl -sSLf https://cursor.com/docs/sitemap.xml \
    | grep -oE '<loc>https://cursor\.com/docs/[^<]+' | sed 's|<loc>||' | sort -u \
    | sed -E 's|(https://cursor\.com/docs/(.*))|\1.md\t\2.md|' \
    | fetch_list "$dir"
  stamp "$dir" "<https://cursor.com/docs> (the \`.md\` twin of every page in its sitemap)" \
    "Cursor. cctop reads its agent sessions, and its bundled Codex server counts as a Codex install."
}

pull_windsurf() {
  local dir; dir="$(reset_dir windsurf)"
  # Unlike the others, docs.windsurf.com answers a `.md` URL with its
  # single-page app rather than the markdown. Its llms-full.txt is the whole
  # site in one file, so split that back into a page per `Source:` header.
  curl -sSLf https://docs.windsurf.com/llms-full.txt -o "$tmp/windsurf.txt"
  python3 "$root/windsurf-split.py" "$tmp/windsurf.txt" "$dir"
  stamp "$dir" "<https://docs.windsurf.com/llms-full.txt>, split back into a file per page" \
    "Windsurf. It records no tool outcomes, which is why its success column reads a dash."
}

pull_codex() {
  local dir; dir="$(reset_dir codex)"
  from_repo "$dir" https://github.com/openai/codex.git main docs README.md
  stamp "$dir" "<https://github.com/openai/codex> (\`docs/\` and \`README.md\`)" \
    "Codex. cctop reads its rollout files under \`\$CODEX_HOME/sessions\` and its \`auth.json\` accounts."
}

pull_gemini() {
  local dir; dir="$(reset_dir gemini-cli)"
  from_repo "$dir" https://github.com/google-gemini/gemini-cli.git main docs README.md
  stamp "$dir" "<https://github.com/google-gemini/gemini-cli> (\`docs/\` and \`README.md\`)" \
    "Gemini CLI."
}

pull_opencode() {
  local dir; dir="$(reset_dir opencode)"
  from_repo "$dir" https://github.com/anomalyco/opencode.git dev packages/web/src/content/docs README.md
  # Upstream ships the same pages in twenty languages; keep the English ones.
  find "$dir/docs" -mindepth 1 -maxdepth 1 -type d -exec rm -rf {} +
  stamp "$dir" "<https://github.com/anomalyco/opencode> (\`packages/web/src/content/docs\`, English only, and \`README.md\`)" \
    "OpenCode. The translated copies of each page are dropped on the way in."
}

pull_pi() {
  local dir; dir="$(reset_dir pi)"
  from_repo "$dir" https://github.com/earendil-works/pi.git main packages/coding-agent/docs README.md
  stamp "$dir" "<https://github.com/earendil-works/pi> (\`packages/coding-agent/docs\` and \`README.md\`)" \
    "Pi. cctop reads its sessions under \`\$PI_CODING_AGENT_DIR\`, default \`~/.pi/agent\`."
}

all=(claude codex cursor gemini-cli opencode pi windsurf)
for name in "${@:-${all[@]}}"; do
  case "$name" in
    claude) f=pull_claude ;;
    codex) f=pull_codex ;;
    cursor) f=pull_cursor ;;
    gemini-cli|gemini) f=pull_gemini; name=gemini-cli ;;
    opencode) f=pull_opencode ;;
    pi) f=pull_pi ;;
    windsurf) f=pull_windsurf ;;
    *) echo "unknown harness: $name" >&2; exit 2 ;;
  esac
  echo "==> $name"
  "$f"
  echo "    $(find "$root/$name" -type f | wc -l) files"
done
