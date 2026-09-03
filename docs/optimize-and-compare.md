# `cctop optimize` and `cctop compare`

[← back to the README](../README.md)

The table says what a session cost. These two say what it cost you *for*.

```bash
cctop optimize   # what was spent and not got back
cctop compare    # how each model did on the work you gave it
```

Both are also `o` and `c` in the TUI, drawn over the table, and both take
`--json` and `--provider <name>`.

Neither writes anything — not to your configuration, not anywhere. They read
transcripts and print.

## Why they are slower than everything else

They re-read every transcript. The individual tool calls, with their arguments,
are the thing both commands reason about, and those are
[never cached](providers/README.md) — at roughly 31 KB a session they were 83%
of a cache that had to be read in full before the first frame. So the table gets
a cache that stays small and these get a full parse, which takes a second or two
on a large machine.

## What `optimize` looks for

Findings come in three classes, and the class matters more than the wording:

| | |
|---|---|
| **fix** | Something to go and do — a setting, a deny rule |
| **habit** | Only you can change it |
| **note** | Worth knowing. Not a criticism |

Each carries what it cost, and whether that figure was **measured** — counted
from tokens the transcript recorded — or **estimated** from this machine's own
averages. The distinction is not decoration. A measured saving is one you can
check; an estimated one is an argument.

What it currently detects:

- Reads into generated or vendored directories — `node_modules`, `.git`,
  build output
- The same file read twice inside one session, which is usually a compaction
  that took it out of the window
- One file read from scratch by five or more separate sessions — a piece of
  context the agent needs every time and is told nowhere
- Sessions that read ten times more than they edited, excluding the ones whose
  job was to explore
- Tool calls that failed and were billed anyway
- Sessions that spent real money and changed no file

Underneath, where the money went by kind of work: coding, debugging, testing,
exploration, planning, delegation, git, build, conversation.

**The headline only counts what could actually be recovered.** A `note` records
what a set of sessions *spent*, which is an observation and not a saving — an
earlier version added them together and advertised $218 of ordinary work as
though it were waste.

## What `compare` measures

Per model, and then per model per kind of work:

| | |
|---|---|
| **1-shot** | Share of files that took one contiguous attempt |
| **$/file** | Cost per file actually changed |
| **$/call** | Cost per tool call |
| **cache** | Share of input that came from the cache |

The one worth understanding is **1-shot**, because of how a retry is counted.
Editing a file, going away to run something, and editing that same file again is
a retry. Editing a *different* file is progress, not a retry. That makes it a
sharper signal than `ERR%`: a failed call is noise, an agent editing one file
four times is a story.

### It is observational, and it says so

You did not give two models the same work. You gave the expensive one the
problems you expected to be hard. A table that ignores that reports the
expensive model as worse while measuring nothing but your own routing.

Nothing can fix that from a transcript, so two things make it visible instead:
the caveat is printed under every table, and the same figures are broken out per
kind of work — most of "this model is worse" turns out to be "this model was
given the debugging".

A session that used several models is credited entirely to whichever cost the
most. The transcript records which model billed a request, not which model asked
for a given tool call.

## Where the numbers are floors

The per-session tool history is capped, so a session that made more calls than
the cap has its oldest ones dropped. Any count derived from it is therefore a
floor, and a finding built on a capped session says so on its own line rather
than quietly under-reporting.

## What is deliberately missing

**Applying fixes.** `optimize` tells you what to change and does not change it.
Writing to somebody's `~/.claude/` is a different kind of commitment from
reading it, and it should not arrive in the same release as the detectors that
decide what to write. The findings come first; automating the ones that turn out
to be right can follow.

**Config scanning.** Nothing yet reads `CLAUDE.md`, MCP server definitions, or
agent and skill files to find the ones that are never used. Those detectors need
a different source of truth from the transcripts and are the obvious next step.
