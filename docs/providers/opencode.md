# OpenCode

[← all harnesses](README.md) · parser: [`src/session/opencode.rs`](../../src/session/opencode.rs)

The first of the two harnesses that keep everything in one database rather than
one file per session. That single fact is behind most of what is unusual here:
the caching key, the connection pool, and why the mtime of the file on disk is
useless as a freshness signal.

## Where it lives

`$OPENCODE_DATA_DIR`, defaulting to the platform data directory plus `opencode`
(`~/.local/share/opencode` on Linux). Any file matching `opencode*.db` in it is
read: stable releases use `opencode.db` and nonstandard channels suffix the
name, so a machine can hold several. A session migrated between channels appears
in both, and `list_sessions` keeps the most recently updated copy.

Its *configuration* lives somewhere else — `$OPENCODE_CONFIG_DIR`, the platform
config directory — which is where `--install-hooks` writes the plugin, and where
`access.rs` looks for `opencode.json`.

## The format

SQLite, read strictly read-only (`SQLITE_OPEN_READ_ONLY | NO_MUTEX`). Three
tables matter:

| Table | What cctop takes |
|---|---|
| `session` | `id`, `directory`, `title`, `model` (JSON, `.id` inside), `time_created`, `time_updated`, and aggregate `cost` / `tokens_*` columns |
| `message` | `data`, a JSON blob per message: `role`, `modelID`, `tokens{input,output,cache{read,write},reasoning}`, `cost`, `time.created` |
| `part` | `data`, a JSON blob per message part: `type: "tool"`, `tool`, `state{input,status,time{start,end}}`, `callID` |

Discovery reads the `session` table alone, so it stays a single query per
database however many sessions are in it.

## Traps

**`cost: 0` does not mean free.** OpenCode computes a cost for providers it has
rates for and writes zero for the rest — a local proxy, a gateway, any
OpenAI-compatible endpoint. Taken at face value that reads as "this was free",
so `assistant_usage` falls back to pricing the tokens against LiteLLM
(`FallbackRates`) whenever the reported cost is zero *and* there were tokens.
A cost OpenCode did compute is authoritative and is never second-guessed.

**The file's mtime says nothing about a session.** Every session shares one
WAL-backed database, so its size and mtime move when *any* of them is written.
`cache::disk_key` therefore keys OpenCode on the session's own id plus the
`time_updated` discovery already read — the one stamp that moves for the session
that changed and no other. Before that, OpenCode sessions went uncached
entirely: on a 2019-session machine, 1478 of them were OpenCode and they were
84% of all extraction time, repeated in full on every run.
`session::effective_mtime_ms` makes the same choice for the same reason.

**Zeroed turns sit at the end of many sessions.** A turn aborted or failed
before the provider answered is still recorded with every count at zero, so
`context_of` walks back up to 40 assistant rows rather than reading the newest
one and giving up.

**Connections are pooled per thread, and never revalidated.** Every session in a
database is asked about separately — its dot, its window, its last tool — and
each of those used to open its own connection: several thousand opens of a
555 MB database per walk, costing 240,000 context switches and nine seconds of
kernel time, almost all of it in file locking. They are thread-local rather than
shared because `NO_MUTEX` forbids cross-thread use and a shared connection
behind a mutex would serialise the one path that most needs not to be.
*ponytail:* a connection is held for the life of the thread, so a database
swapped out wholesale underneath cctop is read from the old file until restart.

**`write` cannot report what it removed.** `tool_delta` counts lines for `edit`
from the old and new strings and parses `apply_patch` properly, but a `write`
that replaces a file wholesale leaves no record of what was there, so no removal
count is guessed.

## Cost

Reported — the only figures on the table that are not cctop's arithmetic, apart
from Pi's — with the LiteLLM fallback above for the turns OpenCode priced at
nothing. If no assistant message carries usage at all, the `session` row's
aggregate columns stand in. See [What the cost figures mean](../costs.md).

## Context

`extract_context` sums the newest usable assistant message's `tokens.input`,
`tokens.cache.read` and `tokens.cache.write`: what the next turn has to resend,
cached or not. Output is excluded because the next input count already contains
it.

The ceiling is **LiteLLM's `max_input_tokens` for the model, or nothing**.
OpenCode records no window size of its own, and a guessed denominator would
misreport CTX% for every model that did not match it — so a model LiteLLM has
never heard of shows a blank context cell. That is the qualification behind the
README's `✓`.

## Liveness

`opencode --session <id>` (or `-s`) puts the id on the command line, so the
match is exact — `proc::session_value`. The binary may be `opencode`,
`opencode-cli`, or `node` running a script whose stem is `opencode`.

Activity state does not come from a transcript tail, because there is no
transcript: `extract_activity_state` reads the newest `message` row instead. An
assistant message carrying an `error` whose text matches a list of transport
failures — timeout, rate limit, overloaded, connection, queue is full — is an
API error; a message with a `part` naming an input-request tool is waiting on
you.

`delete` is the one destructive path that writes to a harness's own store:
`DELETE FROM session` with foreign keys on, which takes the messages and parts
with it.
