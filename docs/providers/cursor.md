# Cursor

[← all harnesses](README.md) · parser: [`src/session/cursor.rs`](../../src/session/cursor.rs)

Cursor's native agent exports a transcript with the conversation in it and none
of the accounting. Four of the five capability columns are blank for it, and
the parser's module doc states the rule that produces them: keep those fields
unavailable rather than estimating them from incomplete evidence.

## Where it lives

`$CURSOR_HOME/projects/<project>/agent-transcripts/**/*.jsonl`, defaulting to
`~/.cursor` (`config::CURSOR_HOME`). Two filters decide what counts:

- the file stem must be a full UUID, and
- the path must contain an `agent-transcripts` component.

The second is not redundant. `rglob` sweeps the whole projects tree, and only
that subtree is a documented session store; without the check, any UUID-named
JSONL Cursor happens to write becomes a row.

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

**The label is a project slug, not a path.** `project_slug` takes the directory
name under the projects root. Every other harness records its working directory
and cctop shows that; here there is nothing to show, so the row carries Cursor's
own name for the project.

## What cannot be extracted

No model, no tokens, no context, no cost, and no per-call outcome. `summarize`
sets `cost_available = false` and `total_cost = None`, so the money columns read
as unavailable rather than as `$0.00` — the two mean very different things, and
a zero would be a claim. `Provider::records_tool_outcomes` is false, so `ERR%`
is `─` rather than a reassuring `0%`.

Tool counts are the one real signal, which is why the capability table's Tools
column is a `✓`.

## Liveness

Cursor runs one shared editor process, not one process per agent transcript, so
there is nothing to attribute a PID to. `loader::attach_processes` infers
liveness instead: a transcript whose mtime is within 90 seconds is running.
That is why the README's table says *inferred* rather than `✓`, and why the CPU
and memory columns stay blank — showing the editor's own figures on a session
row would attribute one agent's cost to all of them.

There is no resume command either: `Session::resume_argv` returns `None`,
because the conversation lives inside the editor and no CLI invocation picks one
back up. Callers show the transcript path instead of inventing a flag.

## What cctop can still do

`--install-hooks` writes `~/.cursor/hooks.json` (a flat shape, unlike Claude's
and Gemini's nested one), so a live Cursor session can report the states its
transcript cannot express. And `access.rs` lists the project's own
`.cursorrules`-style file with a note saying the rest — models, settings — lives
in the editor UI where nothing can read it.
