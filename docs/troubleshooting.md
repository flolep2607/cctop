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
pricing, whether the parsers are reading what they found (below), the cache and
whether it is writable, the hooks report, and which of the three backends
behind `s` this machine actually has.

`cctop doctor --host devbox` adds a section that makes the ssh round trip for
real, which is the only honest test of it — and reports ssh's own words back
with the fix that matches them, since a key that needs a passphrase and a
hostname that will not resolve need very different answers.

It exits `0` when nothing is broken, `1` for a real fault — an unwritable
cache, no pricing at all, a `--host` that could not be read — and `2` for a bad
argument. A warning is something you chose not to set up, so it does not fail
the exit code and `cctop doctor` is usable in a script to mean "is this
installation sound".

### The Parsers section

A parser that stops understanding its harness rarely errors. It fails closed:
a failure it cannot read counts as a success, a model it cannot price costs
`$0.00`, and both look like good news. Codex sessions reported no tool errors
for months that way. No single session shows it, so this section adds up every
session cctop can see and flags totals too unlikely to be chance:

```
Parsers
  ! Codex                  4,812 tool calls and not one failed — the parser is probably not reading failures, so its error rates read 0%
      → a real agent fails a few percent of its calls; if this harness updated recently, report it at https://github.com/flolep2607/cctop/issues
  ! unpriced model         some-new-model (OpenCode): 4 session(s), 12.3M tokens at $0.00
      → no price table lists these, so their cost is missing, not free; LiteLLM's table here is 3h 12m 5s old, and a new model is priced once LiteLLM adds it
  ✓ Claude Code            27 session(s), 6,640 calls, 3.4% failed, 1.6G tokens
  ✓ free model             space-bunny-free (OpenCode): 2 session(s), 80.8M tokens at $0.00, named as free
```

- **No failures seen.** 500 calls or more from a harness that records outcomes,
  and none failed. Agents fail a few percent of their calls in ordinary use; at
  even 1%, 500 clean calls in a row happens less than 1% of the time. Pi,
  Cursor and Windsurf write no outcome, so they are never flagged.
- **Mostly failures.** More than half of 500 or more calls failed. That is a
  parser reading successes as failures, not an agent having a bad week.
- **No calls seen.** Twenty or more sessions that used tokens, and not one tool
  call among them. For Cursor and Windsurf, which record no tokens, every
  session counts.
- **No tokens seen.** Twenty or more sessions that made tool calls, and not a
  token between them, from a harness that records tokens.
- **Unpriced models.** Any model that used 10,000 tokens or more and cost
  exactly `$0.00`, sorted by tokens. Each one is reported as one of three
  things:
  - a *free model*: LiteLLM lists it at zero, or its name ends in `-free` or
    `:free`. Nothing to fix.
  - an *unpriced model*: no table has a rate, so its cost is unknown, not zero.
    A model newer than LiteLLM's table gets a price once LiteLLM lists it.
  - *price not applied*: a table has a rate and the session still came to
    zero. That is a cctop bug. `cctop --clear-cache` re-prices everything, and
    if it stays at zero, please report it.

Everything here is a warning, never a failure. These are inferences from your
data, not faults in the installation, and the most common one is LiteLLM being
a day behind a model release. So they do not change the exit code. The section
reads the same cached walk `cctop --list` does, so it costs about as much as
`--list`.

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

If only some sessions read `$0.00`, the table loaded and a model in it has no
price. The Parsers section of `doctor` names the model and says whether it is
free, missing from the table, or priced and not applied.

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
