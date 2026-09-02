# Pi

[← all harnesses](README.md) · parser: [`src/session/pi.rs`](../../src/session/pi.rs)

The simplest of the seven to read, and the one that records its own costs most
completely. What it does not record is anything about the context window or the
outcome of a tool call, and those two gaps are the whole of its `─` column.

## Where it lives

`$PI_CODING_AGENT_SESSION_DIR`, or `$PI_CODING_AGENT_DIR/sessions`, or
`~/.pi/agent/sessions` — checked in that order (`config::PI_SESSIONS_ROOT`).
Two variables rather than one because Pi lets the session store move
independently of the agent directory. Everything below the root is swept with
`rglob(root, ".jsonl")`.

## The format

JSONL, with three record types cctop reads:

- `session` — `id`, `cwd`, `timestamp`. Both the id and the start time come from
  here, and a file without one is skipped entirely.
- `session_info` — `name`, which becomes the row's title.
- `message` — `message.role`, `message.model`, `message.timestamp` (epoch
  milliseconds), `message.usage`, and `message.content[]` blocks of type
  `toolCall`.

`usage` carries `input`, `output`, `cacheRead`, `cacheWrite`, `cacheWrite1h`,
`reasoning`, `totalTokens`, and a nested `cost` object with `input`, `output`,
`cacheRead`, `cacheWrite` and `total`. The 1h cache-write figure is a subset of
`cacheWrite`, so the 5m bucket is the remainder — `.min(cache_write)` guards
against a file that says otherwise.

**Discovery parses the whole file**, unlike every other JSONL harness here,
which reads a bounded head and tail. There is no header record that carries the
last-activity time, so the last message's timestamp has to be found by reading
to the end. It is worth knowing if Pi sessions ever get long: this is the one
discovery path whose cost grows with transcript size.

## Cost

Reported, and preserved exactly rather than re-derived from the model id. The
one exception is a turn Pi priced at nothing while reporting tokens — what a
provider it has no rates for produces — which is priced from LiteLLM instead,
since reporting real spend as free hides it. Same rule as OpenCode's, same
`FallbackRates` helper. See [What the cost figures mean](../costs.md).

## What cannot be extracted

**Context is `─`.** `loader::read_tail` has no context reader for Pi. The
transcript records what each turn consumed but never how full the window stood
afterwards, and there is no ceiling recorded either.

**Tool outcomes are `─`.** Pi records the call — name, arguments, id — and
nothing about how it ended. `Provider::records_tool_outcomes` is false for Pi
precisely so `ERR%` reads as unavailable: a transcript with no failures in it
and a transcript that cannot express one look identical, and drawing `0%` would
put a reassuring figure on a row that earned nothing of the sort.

## Liveness

`pi --session <id>` puts the id on the command line, so the match is exact
(`proc::session_value`), and `node` running a script whose stem is `pi` counts
as well. `pi --session <id>` is also the resume command.

Waiting-for-input is read off the tail like the file harnesses: a `message`
record whose content holds a `toolCall` named for an input request
(`session::is_input_request_tool`).

cctop installs no hooks into Pi — it is not one of `hook::HARNESSES` — so
everything on a Pi row is forensic.
