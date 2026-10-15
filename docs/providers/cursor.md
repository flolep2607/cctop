# Cursor

[← all harnesses](README.md) · parser: [`crates/core/src/session/cursor.rs`](../../crates/core/src/session/cursor.rs)

Cursor's native agent exports a transcript with the conversation in it and none
of the accounting. The editor's own database adds the title, folder, model and
context; tokens and cost are recorded nowhere, and the parser's module doc
states the rule that keeps them blank: unavailable rather than estimated from
incomplete evidence.

## Where it lives

`$CURSOR_HOME/projects/<project>/agent-transcripts/<id>/<id>.jsonl`, defaulting
to `~/.cursor` (`config::CURSOR_HOME`). Only each project's `agent-transcripts`
directory is walked — the rest of a project is `mcps/` and `canvases/` with
whole `node_modules` trees — and the file stem must be a full UUID.

**A subagent is not a session.** Its transcript sits at
`<id>/subagents/<sub>.jsonl` and is folded into its parent (see below).
Cursor sometimes keeps a top-level copy of a subagent too, as
`<sub>/<sub>.jsonl`; any id seen under a `subagents/` directory is never a row.

**The same transcript can exist under two project slugs.** Cursor retains a
copy when a project is moved or reopened elsewhere. The UUID stays the stable
identity, so `list_sessions` sorts by activity and keeps the first copy of each
id — newest wins.

## The format

JSONL of `message` records whose `content` is an array of blocks. cctop reads
exactly one kind: blocks with `type: "tool_use"`, for their `name` and `input`.
Nothing else in the file is parsed.

There are **no per-event timestamps**, so tool details are pushed with an empty
`ts` and the Tools panel cannot order or time them. The session's own times come
from the filesystem: `created` for the start (falling back to mtime, since not
every platform records a creation time) and mtime for last activity.

**The label is a project slug unless the database knows the folder.**
`project_slug` takes the directory name under the projects root, which turns `/`
and `_` alike into `-` and so is not a path. When the editor's database records
the chat's workspace folder and that folder exists here, the row carries the
real path instead (see below).

## The editor's database

Cursor's global `state.vscdb` keeps, under the transcript's own id, what the
transcript leaves out. It lives in `$CURSOR_USER_DIR/globalStorage/` (default
`~/.config/Cursor/User`) and, under WSL, in each
`/mnt/c/Users/*/AppData/Roaming/Cursor/User/` — Cursor runs on Windows and its
agent writes transcripts on the Linux side. `config::cursor_global_dbs` finds
them once per process.

- **At list time**, one query over `composerHeaders` gives each row its title,
  start time and workspace folder.
- **At extraction**, a point lookup of `composerData:<id>` in `cursorDiskKV`
  gives the model (Cursor's Auto router, `default`, shows as `auto` and never
  reaches pricing), the reasoning effort, and each subagent's type.
- **For the context column**, the same key's `promptTokenBreakdown`: Cursor's
  own count of the prompt it last sent against the window it allows.

On a Windows drive the database is opened `immutable=1`: a plain read-only open
over 9P creates `-shm`/`-wal` files in Cursor's directory and takes locks the
Windows writer cannot see. Immutable writes nothing, but reads only what Cursor
has checkpointed, so a live chat's model or context can trail by a checkpoint.
Every failure — no database, an older Cursor without `composerHeaders`, a read
torn by a checkpoint — leaves the row as the transcript alone makes it.

## Subagents

`<id>/subagents/*.jsonl` are the session's subagents: their calls count toward
the session's tools and edits, each one is listed with its own tool count, and
their writes keep the parent live. Their type comes from the database; their
model and cost are not recorded, so they read `?` and `—`.

## What cannot be extracted

No tokens, no cost, and no per-call outcome — the database records none
either: every message's `tokenCount` is zero and every `usageData` empty. `summarize`
sets `cost_available = false` and `total_cost = None`, so the money columns read
as unavailable rather than as `$0.00` — the two mean very different things, and
a zero would be a claim. `Provider::records_tool_outcomes` is false, so `ERR%`
is `─` rather than a reassuring `0%`.

Tool counts are the transcript's real signal, which is why the capability
table's Tools column is a `✓`.

## Liveness

Cursor runs one shared editor process, not one process per agent transcript, so
there is nothing to attribute a PID to. `loader::attach_processes` infers
liveness instead: a transcript whose mtime is within 90 seconds is running.
That is why the README's table says *inferred* rather than `✓`, and why the CPU
and memory columns stay blank — showing the editor's own figures on a session
row would attribute one agent's cost to all of them.

What a row is doing comes from the transcript's tail. Cursor ends every turn
with `{"type":"turn_ended","status":…}`, so a finished turn reads as waiting for
you; `status: "error"` is an API error unless its text says the user aborted or
cancelled, which is how Cursor files a Stop. An `AskQuestion` call reads as a
question waiting on you.

There is no resume command either: `Session::resume_argv` returns `None`,
because the conversation lives inside the editor and no CLI invocation picks one
back up. Callers show the transcript path instead of inventing a flag.

## What cctop can still do

`--install-hooks` writes `~/.cursor/hooks.json` (a flat shape, unlike Claude's
and Gemini's nested one), so a live Cursor session can report the states its
transcript cannot express. And `access.rs` lists the project's own
`.cursorrules`-style file with a note saying the rest — models, settings — lives
in the editor UI where nothing can read it.
