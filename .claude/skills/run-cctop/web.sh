#!/usr/bin/env bash
# Drive cctop's *browser* side: serve the fixture, open pages, screenshot them,
# and fake the failures a real tunnel produces.
#
# `driver.sh` drives the TUI through tmux and cannot see any of this — the
# dashboard page, the session report, the conversation view and the action
# routes are a second interface with its own bugs. This is that interface's
# driver.
#
# Everything runs against the same throwaway $HOME `driver.sh fixture` writes,
# so a run never serves the operator's real sessions.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
FIXTURE="${CCTOP_FIXTURE_HOME:-/tmp/cctop-drive/home}"
SHOTS="${CCTOP_SHOTS:-/tmp/cctop-drive/shots}"
STATE="${CCTOP_WEB_STATE:-/tmp/cctop-drive/web}"
BIN="$ROOT/target/debug/cctop"
PORT="${CCTOP_WEB_PORT:-7799}"

say() { printf '\033[36m▶ %s\033[0m\n' "$*" >&2; }
die() { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

# The headless Chromium playwright downloaded, whichever build it is.
#
# The python package pins a build number and errors out when the one it wants
# is missing — but any recent build drives fine, so this finds the newest that
# is actually on disk and hands it over as `executable_path`. Without this the
# usual failure is "Executable doesn't exist at …chromium_headless_shell-1169"
# on a machine that has 1194 sitting next to it.
chromium() {
  # `|| true` because one of the two globs is normally unmatched, which makes
  # `ls` exit 2 — and under `set -o pipefail` that becomes the pipeline's
  # status, which `set -e` then treats as a reason to kill the script. The
  # symptom is the whole command exiting 2 with nothing on either stream.
  { ls -d "$HOME"/.cache/ms-playwright/chromium_headless_shell-*/chrome-linux/headless_shell \
          "$HOME"/.cache/ms-playwright/chromium-*/chrome-linux/headless_shell 2>/dev/null \
      || true; } | sort -V | tail -1
}

cmd_serve() {
  [ -x "$BIN" ] || (cd "$ROOT" && cargo build)
  [ -d "$FIXTURE" ] || "$HERE/driver.sh" fixture >/dev/null
  cmd_down 2>/dev/null || true
  mkdir -p "$SHOTS" "$(dirname "$STATE")"

  local args=(serve --port "$PORT" --no-token)
  # A token is the real shape, but every URL in a shell then needs `?t=`
  # threaded through it. `--no-token` keeps the recipes short; pass --token
  # when the thing under test *is* the token.
  [ "${1:-}" = "--token" ] && args=(serve --port "$PORT")
  [ "${1:-}" = "--tunnel" ] && args=(serve --port "$PORT" --tunnel)

  say "cctop ${args[*]} (HOME=$FIXTURE)"
  env HOME="$FIXTURE" CI=1 "$BIN" "${args[@]}" > "$STATE.log" 2>&1 &
  echo $! > "$STATE.pid"

  # Wait for the port rather than sleeping: a tunnel run takes seconds longer
  # than a local one, and both are ready when the socket answers.
  local url="http://127.0.0.1:$PORT"
  for _ in $(seq 1 60); do
    curl -s -o /dev/null -m 1 "$url/" && break
    sleep 0.5
  done
  # The token, when there is one, is only ever printed — so it is read back off
  # the announcement rather than guessed.
  local token=""
  token="$(grep -oE '\?t=[a-f0-9]+' "$STATE.log" | head -1 | cut -d= -f2 || true)"
  local query=""
  [ -n "$token" ] && query="?t=$token"
  echo "$url$query" > "$STATE.url"
  grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "$STATE.log" | head -1 > "$STATE.tunnel" || true
  # The socket answers before the first walk finishes, so a run that starts
  # asking straight away sees an empty table and reads it as "the fixture is
  # missing". Wait for a row, not for the port.
  local ready=""
  for _ in $(seq 1 60); do
    if curl -s -m 2 "$url$query/api/sessions" | grep -q '"session_id"'; then
      ready=1; break
    fi
    sleep 0.5
  done
  [ -n "$ready" ] || say "warning: 30s and the table is still empty"
  say "serving $url$query"
  [ -s "$STATE.tunnel" ] && say "tunnel $(cat "$STATE.tunnel")$query"
  cat "$STATE.url"
}

# The URL of a page under the running server, with the token already on it.
page_url() {
  [ -f "$STATE.url" ] || die "nothing is being served — run: web.sh serve"
  local base; base="$(cat "$STATE.url")"
  local path="${1:-/}"
  case "$base" in
    *\?t=*) echo "${base%%\?*}$path?t=${base##*\?t=}" ;;
    *)      echo "${base%/}$path" ;;
  esac
}

cmd_api() {
  local url; url="$(page_url "${1:?usage: web.sh api /api/sessions}")"
  curl -s "$url" | python3 -m json.tool
}

# Every session id the fixture holds, for building report URLs.
cmd_ids() {
  local url; url="$(page_url /api/sessions)"
  curl -s "$url" | python3 -c 'import sys,json;[print(r["session_id"], r.get("provider",""), r.get("project","")) for r in json.load(sys.stdin)]'
}

# Screenshot a page and print the text it rendered.
#
# The text is the half worth reading in a transcript — a PNG says "something is
# there", the text says what. Both land in $SHOTS.
cmd_shot() {
  local name="${1:?usage: web.sh shot <name> [path] [--dead]}"
  local path="${2:-/}"
  local dead=""
  [ "${3:-}" = "--dead" ] && dead=1
  local url; url="$(page_url "$path")"
  local browser; browser="$(chromium)"
  [ -n "$browser" ] || die "no playwright chromium on this machine (pip install playwright && playwright install chromium)"
  mkdir -p "$SHOTS"
  say "$url"
  CCTOP_URL="$url" CCTOP_OUT="$SHOTS/$name" CCTOP_BROWSER="$browser" CCTOP_DEAD="$dead" \
    python3 "$HERE/shot_web.py"
}

# Make the server fail on demand — needs `cargo build --features debug`.
#
#   web.sh fault 502     a tunnel whose far end has gone (HTML body, 502)
#   web.sh fault html    a 200 that is not JSON: a captive portal, a proxy
#   web.sh fault empty   a 200 with no body at all
#   web.sh fault slow    a request that never arrives in time
#   web.sh fault off
#
# Faults apply to `/api/**` and not to the pages, so the page under test still
# loads and it is the fetch behind it that breaks — which is the failure worth
# reproducing. `--dead` on `web.sh shot` does the 502 case in the browser
# instead, and needs no special build.
cmd_fault() {
  local mode="${1:-off}"
  local url; url="$(page_url /api/debug/fault)"
  local sep="?"; case "$url" in *\?*) sep="&";; esac
  local out; out="$(curl -s "$url$sep" -G --data-urlencode "mode=$mode")"
  case "$out" in
    *fault*) say "$out" ;;
    *) die "no debug routes in this build — cargo build --features debug" ;;
  esac
}

cmd_state() {
  curl -s "$(page_url /api/debug/state)" | python3 -m json.tool
}

cmd_down() {
  if [ -f "$STATE.pid" ]; then
    kill "$(cat "$STATE.pid")" 2>/dev/null || true
    rm -f "$STATE.pid" "$STATE.url" "$STATE.tunnel"
  fi
  pkill -f "cctop serve --port $PORT" 2>/dev/null || true
}

# Give the fixture's conversation something worth rendering.
#
# The bare fixture holds two turns of plain text, which exercises none of the
# conversation view: markdown, tables, tool calls, a slash command, a reminder.
# Appended rather than rewritten so `driver.sh fixture` stays the one source of
# the fixture's shape.
cmd_chat() {
  [ -d "$FIXTURE" ] || "$HERE/driver.sh" fixture >/dev/null
  python3 "$HERE/enrich_chat.py" "$FIXTURE"
}

cmd_smoke() {
  cmd_serve >/dev/null
  local id; id="$(cmd_ids | head -1 | cut -d' ' -f1)"
  [ -n "$id" ] || die "the served table is empty — is the fixture there?"
  say "dashboard"
  cmd_shot web-dashboard / >/dev/null
  say "session $id"
  cmd_shot web-session "/session/$id" >/dev/null
  say "the same page with its server gone"
  cmd_shot web-dead "/session/$id" --dead >/dev/null
  cmd_down
  say "WEB SMOKE OK — shots in $SHOTS"
}

case "${1:-}" in
  serve) shift; cmd_serve "$@" ;;
  api)   shift; cmd_api "$@" ;;
  ids)   shift; cmd_ids "$@" ;;
  shot)  shift; cmd_shot "$@" ;;
  chat)  shift; cmd_chat "$@" ;;
  fault) shift; cmd_fault "$@" ;;
  state) shift; cmd_state "$@" ;;
  down)  shift; cmd_down "$@" ;;
  smoke) shift; cmd_smoke "$@" ;;
  url)   page_url "${2:-/}" ;;
  *) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
     echo
     echo "usage: web.sh serve [--token|--tunnel] | api <path> | ids | url [path]"
     echo "       web.sh shot <name> [path] [--dead] | chat | smoke | down"
     echo "       web.sh fault 502|html|empty|slow|off | state   (needs --features debug)"
     ;;
esac
