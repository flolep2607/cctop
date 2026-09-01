#!/usr/bin/env bash
# Drive cctop from an agent: build it, give it something to show, run it in
# tmux, press keys, read the screen back.
#
# cctop is a dashboard over *other people's* agent sessions, so on a clean
# machine it draws an empty table and there is nothing to verify. `fixture`
# writes a throwaway $HOME holding transcripts for several harnesses — that,
# not the binary, is what makes a run meaningful.
#
# Everything runs against that fixture home by default, so a run never reads
# the operator's real sessions and never edits their shell rc files.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
SESSION="${CCTOP_TMUX_SESSION:-drive}"
# A tmux server of our own. cctop shells out to tmux for the tab bar and
# follows $TMUX, so a private socket is what keeps it from adopting the
# machine's real cctop sessions as tabs — see `up --spawn` and Gotchas.
SOCKET="${CCTOP_TMUX_SOCKET:-cctopdrv}"
TM=(tmux -L "$SOCKET")
FIXTURE="${CCTOP_FIXTURE_HOME:-/tmp/cctop-drive/home}"
SHOTS="${CCTOP_SHOTS:-/tmp/cctop-drive/shots}"
BIN="$ROOT/target/debug/cctop"
COLS="${CCTOP_COLS:-200}"
ROWS="${CCTOP_ROWS:-50}"

say() { printf '\033[36m▶ %s\033[0m\n' "$*" >&2; }

# Wait for text to appear on screen. Beats `sleep N`: returns as soon as the
# frame is there, and says what it was waiting for when it never arrives.
wait_for() {
  local needle="$1" secs="${2:-20}"
  if ! timeout "$secs" bash -c '
        until tmux -L "$0" capture-pane -t "$1" -p 2>/dev/null | grep -qF -- "$2"; do sleep 0.2; done
      ' "$SOCKET" "$SESSION" "$needle"; then
    echo "TIMEOUT waiting for: $needle" >&2
    "${TM[@]}" capture-pane -t "$SESSION" -p 2>/dev/null | tail -25 >&2 || true
    return 1
  fi
}

cmd_build() {
  say "cargo build (debug)"
  cd "$ROOT" && cargo build
}

cmd_fixture() {
  say "fixture home → $FIXTURE"
  rm -rf "$FIXTURE"
  local proj="$FIXTURE/repo"
  # Claude Code keeps one directory per project, named after the path with the
  # separators flattened. Getting this slug wrong is the usual reason a
  # hand-made fixture shows a row with no project.
  local slug; slug="$(printf '%s' "$proj" | tr '/.' '--')"
  mkdir -p "$FIXTURE/.claude/projects/$slug" "$proj"
  local ts; ts="$(date -u +%Y-%m-%dT%H:%M:%S.000Z)"
  cat > "$FIXTURE/.claude/projects/$slug/11111111-2222-3333-4444-555555555555.jsonl" <<EOF
{"type":"user","timestamp":"$ts","cwd":"$proj","message":{"role":"user","content":"count the lines"}}
{"type":"assistant","timestamp":"$ts","requestId":"req_1","message":{"id":"m1","role":"assistant","model":"claude-opus-5","content":[{"type":"text","text":"working"}],"usage":{"input_tokens":42000,"output_tokens":900}}}
EOF
  # Two Codex accounts: `auth.json` is what makes a directory an account, so
  # this is also what makes the launcher offer a choice and the PROFILE column
  # appear. Tokens are nonsense — nothing here talks to a network.
  for home in .codex .codex-work; do
    mkdir -p "$FIXTURE/$home/sessions/2026/08/20"
    echo '{"tokens":{"access_token":"fixture"}}' > "$FIXTURE/$home/auth.json"
  done
  cat > "$FIXTURE/.codex-work/sessions/2026/08/20/rollout-2026-08-20T10-00-00-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl" <<EOF
{"type":"session_meta","payload":{"id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","timestamp":"$ts","cwd":"$proj","originator":"codex_cli_rs","cli_version":"0.146.1"}}
{"type":"turn_context","payload":{"cwd":"$proj","model":"gpt-5.6-terra"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":31200,"cached_input_tokens":0,"output_tokens":1340,"total_tokens":32540},"last_token_usage":{"input_tokens":31200,"cached_input_tokens":0,"output_tokens":1340,"total_tokens":32540}}}}
EOF
  # Without a pricing table every model prices at $0.00, which is
  # indistinguishable from a free plan — so borrow the operator's cached copy
  # when there is one. It is the public LiteLLM table, not anything personal.
  local cache="${XDG_CACHE_HOME:-$HOME/.cache}/cctop/litellm-pricing.json"
  if [ -f "$cache" ]; then
    mkdir -p "$FIXTURE/.cache/cctop" && cp "$cache" "$FIXTURE/.cache/cctop/"
    echo "  (seeded pricing cache from $cache)"
  else
    echo "  (no pricing cache to seed — costs will read \$0.00 until cctop fetches one)" >&2
  fi
  mkdir -p "$SHOTS"
  find "$FIXTURE" -name '*.jsonl' -o -name auth.json | sed "s|$FIXTURE|~|"
}

# cctop's own env, for anything run outside tmux.
fixture_env() {
  # CI=1 skips the first-run prompt that offers to write shell rc files.
  printf 'HOME=%s CI=1 NO_COLOR=%s' "$FIXTURE" "${NO_COLOR:-}"
}

cmd_up() {
  [ -x "$BIN" ] || cmd_build
  [ -d "$FIXTURE" ] || cmd_fixture
  local launch=("$BIN")
  if [ "${1:-}" = "--spawn" ]; then
    # Let the launcher actually start agents: cctop refuses to nest a
    # `new-session` under a $TMUX/$RMUX it can see, so the wrapper drops them.
    # The cost is that cctop then talks to the machine's *real* rmux server
    # and adopts every cctop-owned session there as a tab.
    #
    # All four, not just the TMUX pair. On a machine where `tmux` is the rmux
    # shim — which is how rmux installs itself — this driver's own server is an
    # rmux server on a private socket, and a pane in it carries RMUX naming
    # that socket. A cctop that inherits it asks the *driver's* server for the
    # cctop-* sessions, finds none, and draws a bar with no tabs; a
    # `set-option` on a real session answers "can't find session". That looked
    # for a while like a cctop bug and was this line.
    printf '#!/bin/sh\nunset TMUX TMUX_PANE RMUX RMUX_PANE\nexec %s\n' "$BIN" > "$SHOTS/../nested.sh"
    chmod +x "$SHOTS/../nested.sh"
    launch=("$SHOTS/../nested.sh")
    say "spawn mode: real tmux server, real sessions will appear as tabs"
  fi
  "${TM[@]}" kill-session -t "$SESSION" 2>/dev/null || true
  say "launching ${launch[*]} on tmux socket '$SOCKET' (${COLS}x${ROWS})"
  # -e, not `HOME=x tmux new-session`: the pane inherits the *server's*
  # environment, and a server is usually already running, so a variable set on
  # the client command line silently does not reach the app.
  "${TM[@]}" new-session -d -s "$SESSION" -x "$COLS" -y "$ROWS" \
    -e "HOME=$FIXTURE" -e "CI=1" -e "TERM=xterm-256color" "${launch[@]}"
  wait_for "Overview" 30
  # In --spawn mode cctop opens *onto* one of the adopted tabs, i.e. inside a
  # live agent's terminal, where every key but the function keys is forwarded
  # to that agent. F12 is the way back to the dashboard, and is sent
  # unconditionally so no caller can send a keystroke into somebody's session.
  "${TM[@]}" send-keys -t "$SESSION" F12 2>/dev/null || true
  # Not the column header — that is drawn while the table still says
  # "Scanning for sessions…", so waiting on it captures a half-loaded screen.
  # A model name means a row has been read off a transcript. Note the MODEL
  # column is *shortened* for display, so the marker has to be a name that
  # survives it: `claude-opus-5` renders as `opus-5` and would never match.
  wait_for "gpt-5.6-terra" 30
  say "up"
}

cmd_keys() { "${TM[@]}" send-keys -t "$SESSION" "$@"; }

cmd_wait() { wait_for "$@"; }

cmd_shot() {
  local name="${1:-screen}"
  mkdir -p "$SHOTS"
  "${TM[@]}" capture-pane -t "$SESSION" -p > "$SHOTS/$name.txt"
  say "$SHOTS/$name.txt"
  cat "$SHOTS/$name.txt"
}

cmd_down() {
  # F10 is quit from anywhere, including inside a pane.
  "${TM[@]}" send-keys -t "$SESSION" F10 2>/dev/null || true
  sleep 0.5
  "${TM[@]}" kill-server 2>/dev/null || true
  say "down"
}

# The headless surfaces: no tmux, no terminal, machine-readable.
cmd_headless() {
  [ -x "$BIN" ] || cmd_build
  [ -d "$FIXTURE" ] || cmd_fixture
  say "cctop doctor  (exits 1 if any check is ✗ — that is a report, not a crash)"
  env HOME="$FIXTURE" CI=1 "$BIN" doctor | sed -n '1,14p' || true
  say "cctop -j (rows as JSON)"
  env HOME="$FIXTURE" CI=1 "$BIN" -j | python3 -c '
import json,sys
d=json.load(sys.stdin); rows=d["sessions"] if isinstance(d,dict) else d
for s in rows:
    print(" ", s.get("provider"), "|", s.get("profile"), "|", s.get("model"), "|", s.get("project"))
print(f"  {len(rows)} row(s)")'
}

cmd_smoke() {
  cmd_build
  cmd_fixture
  cmd_headless
  cmd_up

  # 1. The table drew the fixture rows, with the account each came from.
  cmd_shot dashboard >/dev/null
  for want in "1:Dashboard" "gpt-5.6-terra" "PROFILE" "work"; do
    grep -qF -- "$want" "$SHOTS/dashboard.txt" || { echo "dashboard missing: $want" >&2; exit 1; }
  done

  # 2. The launcher, which is the one screen that reaches outside cctop. Only
  #    inspected, not fired: see `up --spawn` for why Enter here is a dead end.
  cmd_keys M-n; wait_for "New tab" 10
  cmd_keys Down; wait_for "p to change" 10       # `codex`, which has two accounts
  cmd_shot launcher >/dev/null
  cmd_keys p; wait_for "as work" 10
  cmd_shot launcher-work >/dev/null
  cmd_keys Escape

  # 3. The row menu, which is what Enter on a row opens.
  cmd_keys Down; cmd_keys Enter; wait_for "Resume in a tab" 10
  cmd_shot row-menu >/dev/null
  cmd_keys Escape

  # 4. Per-account limits: one column per subscription, each naming its own
  #    harness's login command.
  cmd_shot limits >/dev/null
  grep -qF "Codex (work)" "$SHOTS/limits.txt" || { echo "no per-account limits column" >&2; exit 1; }

  cmd_down
  say "SMOKE OK — shots in $SHOTS"
}

case "${1:-}" in
  build)    shift; cmd_build "$@" ;;
  fixture)  shift; cmd_fixture "$@" ;;
  up)       shift; cmd_up "$@" ;;
  attach)   shift; say "Ctrl-b d to detach"; "${TM[@]}" attach -t "$SESSION" ;;
  keys)     shift; cmd_keys "$@" ;;
  wait)     shift; cmd_wait "$@" ;;
  shot)     shift; cmd_shot "$@" ;;
  down)     shift; cmd_down "$@" ;;
  headless) shift; cmd_headless "$@" ;;
  smoke)    shift; cmd_smoke "$@" ;;
  env)      fixture_env; echo ;;
  *) sed -n '2,12p' "${BASH_SOURCE[0]}"; echo
     echo "usage: driver.sh {build|fixture|up [--spawn]|keys <keys…>|wait <text> [secs]|shot [name]|attach|down|headless|smoke}" ;;
esac
