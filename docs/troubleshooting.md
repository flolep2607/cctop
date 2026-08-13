# Troubleshooting

[← back to the README](../README.md)

## Start here: `cctop doctor`

Most of what can go wrong is invisible from outside the process: a
`CLAUDE_CONFIG_DIR` left over from an experiment sending discovery somewhere
empty, a pricing table that never downloaded so every session reads `$0.00`,
hooks installed against a binary that has since moved. `doctor` prints all of
it — one line per check, with the fix attached to anything that is not fine.

```
Session sources
  ✓ Claude Code            26 session(s)
  ! Cursor                 directory exists but holds no sessions (/home/flo/.cursor/projects)
      → if that is wrong, check the environment overrides above

Pricing
  ! LiteLLM table          cached but 49h 35m old
      → the next interactive run refreshes it; costs use the stale rates until then
```

It covers the version and binary path, any `CLAUDE_CONFIG_DIR`-style overrides
in the environment, every harness's session directory and how many it found,
pricing, the cache and whether it is writable, the hooks report, and which of
the three backends behind `s` this machine actually has.

`cctop doctor --host devbox` adds a section that makes the ssh round trip for
real, which is the only honest test of it — and reports ssh's own words back
with the fix that matches them, since a key that needs a passphrase and a
hostname that will not resolve need very different answers.

It exits `0` when nothing is broken, `1` for a real fault — an unwritable
cache, no pricing at all, a `--host` that could not be read — and `2` for a bad
argument. A warning is something you chose not to set up, so it does not fail
the exit code and `cctop doctor` is usable in a script to mean "is this
installation sound".

## My sessions are missing

Run `cctop doctor` first — the Session sources section names every directory it
looked in and how many it found in each, which answers this outright most of
the time.

The usual causes, in order of how often they turn out to be it:

- **An environment override.** `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `CURSOR_HOME`,
  `GEMINI_DIR`, `OPENCODE_DATA_DIR`, `PI_CODING_AGENT_DIR`,
  `PI_CODING_AGENT_SESSION_DIR` and `WINDSURF_USER_DIR` all move where cctop
  looks. `doctor` lists the ones that are set.
- **The agent has not written a transcript yet.** cctop reads what the harnesses
  leave on disk; a session that has just started may not be there for a moment.
- **A filter is on.** `Esc` clears one layer at a time, and the table's title
  says `Sessions (6/71)` whenever anything is hidden.

## Every session costs $0.00

The pricing table did not load. `doctor` reports this as a failure rather than a
warning, because a missing download and a genuinely free plan look identical in
the `$` column. cctop needs to reach
`raw.githubusercontent.com` once, then caches for 24 hours.

## `s` does nothing

Typing into a session needs cctop to reach the pty the agent is reading from,
and there are only three ways to do that — see
[Driving agents](driving-agents.md#typing-into-a-session). `doctor` reports
which of them this machine has.

The usual fix is `cctop --install-alias`, then starting agents from a shell as
normal.

## A `--host` machine never appears

`cctop doctor --host <host>` makes the ssh round trip and reports ssh's own
words back. The most common cause is that a non-interactive ssh gets a
different `PATH` than a login shell, so `cctop` is not found even though it
works when you ssh in by hand — name the binary instead:

```bash
cctop --host devbox:/usr/local/bin/cctop
```
