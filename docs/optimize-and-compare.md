# `cctop optimize` and `cctop compare`

[← back to the README](../README.md)

The table says what a session cost. These two say what it cost you *for*.

```bash
cctop optimize   # what was spent and not got back
cctop compare    # how each model did on the work you gave it
```

Both are also `o` and `c` in the TUI, drawn over the table, and both take
`--json`, `--provider <name>` and `--since <span>` — `24h`, `7d`, `2w`, or a
date such as `2026-09-01`. Models change, and so does what you give them, so
last month's sessions blur this week's comparison.

Neither writes anything — not to your configuration, not anywhere. They read
transcripts, and `optimize` reads Claude Code's configuration to say where
something it found is defined, and print.

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
- MCP servers, skills, agents and plugins that Claude Code offered to five or
  more sessions and nothing ever used — see
  [below](#what-a-session-was-given-and-never-used)
- What the memory files — every `CLAUDE.md`, and the auto-memory index — cost
  by being re-read on every request

Underneath, where the money went by kind of work: coding, debugging, testing,
exploration, planning, delegation, git, build, conversation.

**A finding has to be worth more than the time it takes to act on.** The report
once opened with a `fix` worth $0.0071, ranked above a habit worth a dollar, and
both figures were right. Neither was worth having: *a dollar is like a few
minutes of my time*, and a fix is several minutes — read the row, find the
settings file, edit it, check it did something. A list whose first row is a
losing trade is a list people stop reading.

So a finding must clear **$5, or 1% of what these sessions spent, whichever is
larger**. The share matters because the absolute floor alone misjudges scale:
$5 back is worth having on a corpus that spent $40 and invisible on one that
spent $5,000, and only the second kind of user is drowning in findings.

What falls below the bar is not silently dropped — it gets one line saying how
many findings there were and what they came to, because a report that detected
four things and printed none of them is indistinguishable from a broken
detector. `--json` carries the same thing as `floor_usd` and `below_floor`, so
a script can reach past the bar without reimplementing the detectors.

A finding with no price on it is kept. Unpriced is not the same as small, and
filtering on the number alone would drop the findings cctop knows least about
while keeping the ones it has measured as trivial. When those are all that is
left, the headline says so rather than claiming `$0.00 looks recoverable`.

**The headline only counts what could actually be recovered.** A `note` records
what a set of sessions *spent*, which is an observation and not a saving — an
earlier version added them together and advertised $218 of ordinary work as
though it were waste.

### What a session was given and never used

Before a Claude Code conversation starts, every skill, agent and MCP tool it
could reach is described in the prompt, along with each `CLAUDE.md` that
applies. That prefix is cached, and read back from the cache on every request
the session makes. Something nobody uses is not free just because it never
runs — though it is usually close to free, which is what the value floor is
for.

"Never used" is the easiest claim here to get wrong, so each half of it comes
from the strictest source there is:

- **Offered** is read from the transcript, never from the configuration.
  Claude Code records the skill listing, the agent listing, the MCP tools it
  offered and the instructions each server sent, and a session only counts
  toward a server if its own listing names it. A server switched off, a
  project `.mcp.json` nobody approved, or one added yesterday is in no listing
  from before then — so it is judged only by the sessions that had it, and
  needs five of them.
- **Used** is any way in: a tool call, an MCP resource read, a slash command,
  the `Skill` tool, the agent opening the skill's own `SKILL.md`, a delegation
  to the agent.
- **Still there** is the configuration as it is now: `~/.claude.json` (user and
  per-project servers, and `disabledMcpServers`), the project's `.mcp.json` and
  `disabledMcpjsonServers` in any settings file, `enabledPlugins`, and the
  skills and agents directories. Something disabled or removed since is not
  reported from the transcripts of before. For a claude.ai connector or a
  synced skill, which live on an account cctop cannot read, the newest session
  stands in: if it was no longer offered the thing, it has been dealt with.

Each finding is one place to act — a file, a plugin, the connectors — and names
it: `claude mcp remove`, `/mcp` to switch a server off for the projects that
never call it, `disabledMcpjsonServers` in `settings.local.json` for a shared
`.mcp.json` (so it stops loading for you without being taken from anyone who
clones the project), `/plugin` for a plugin. A plugin is reported only when
*nothing* it adds was used, since it cannot be removed in part.

Anything cctop cannot trace to a file is never reported: Claude Code's own
built-in skills and agents are in every listing and are not yours to remove.
Only names and paths are printed. An MCP entry can hold an API key in `env` or
a token in `headers`, and no value from a configuration file reaches the
output.

The price is estimated, and how is worth saying: the characters each entry put
in the window are counted from the transcript, turned into tokens at the same
fitted 2.75 characters per token the Context panel uses, multiplied by the
session's requests, and priced at what that session paid per cached token. The
first request writes the prefix rather than reading it, at a higher price, so
the figure runs a little low.

**Memory files are a `note`, and a price, not an accusation.** `CLAUDE.md` is
the cheapest place to tell an agent something — other findings here recommend
putting more in it — so calling its cost waste would contradict the rest of the
report. The one objective line is Claude Code's own: it warns about a memory
file past 40,000 characters, and a file past that is offered as a `habit`, with
only the part beyond the line counted as a saving.

Limits worth knowing:

- Claude Code only, and only versions that write these listings. Older
  transcripts, and every other harness, contribute nothing here.
- An MCP server whose tools load in full rather than through tool search is
  listed nowhere in the transcript, so it is never seen and never reported.
  Its tool definitions are also exactly the expensive case; there is simply no
  record of them to count.
- One profile: sessions run under another `CLAUDE_CONFIG_DIR` are resolved
  against this one's files, where their servers usually are not found — and
  something not found is not reported.
- A subagent's own copy of the listings is not counted, so the cost is low by
  that much again.

## What `compare` measures

Per model, and then per model per kind of work:

| | |
|---|---|
| **files** | Files the model edited; `4+9` adds nine its subagents on another model wrote for it |
| **1-shot** | Share of its files edited without a retry, with a 95% range: `95%±3` |
| **reworked** | Share of its files a later session had to fix within a day |
| **red** | Share of its agents whose last test or build after their final edit failed |
| **$/file** | Its cost per file, delegated ones included |
| **time/file** | Its working time per file, delegated ones included |
| **cache** | Share of input that came from the cache |

With `--rate 60`, a last column prices the time at $60 an hour and adds it to the
dollars, and the table ranks by that. A free model that takes a day per file is
not cheaper than a $10 one that takes five minutes, and dollars alone say it is.
There is no default rate: what an hour of waiting is worth is your number.

### A retry is a second attempt after something failed

Editing a file, seeing a command or an edit fail, and editing that same file
again is a retry. Editing it again after the tests passed is the next step, and
editing a *different* file is progress — neither counts against the model. Only
the same agent's calls are read, because subagents' calls are interleaved into
the same history and one of theirs failing says nothing about the parent's
attempt.

A harness that records no per-call outcome — Cursor, Pi, Windsurf — has no
failure to see, so there any call between two edits of a file makes the second a
retry.

The `±` is a Wilson interval: 100% on seven files is `100%±18`, and two rows
whose ranges overlap are not told apart by this data.

### Did the work hold

1-shot is judged inside one session. **reworked** asks the question it cannot: a
later session reopening the file to fix it, within a day. A later edit is a fix
when it came straight after that session's own command failed, or when the
session is filed as debugging — titles hardly ever say "fix", so a failing
command is the signal that works in any language. The session that wrote the file
last is charged, once. Worktrees under `.claude/worktrees/` count as their main
checkout, or the same file in two worktrees would never match.

**red** is the other end: the agent stopped with its last test or build failing.
Only test and build commands count — a `grep` that found nothing exits non-zero
too — and an agent that checked nothing after its last edit is unknown, not red.

### Subagents are credited to their own model

A session that delegates is several models at once. Each subagent is credited
to its own model with its own cost, calls and 1-shot rate. Responsibility for
the result is shared, though: a parent that briefs a subagent, waits for it and
uses what came back did real work towards those files, and paid for it. So the
parent's cost and time are spread over its own files *and* the delegated ones,
while its 1-shot and rework rates stay about its own writing.

The parent's clock is the session's end to end, since it was waiting while its
subagents worked. A subagent on the parent's own model lands in the same row and
adds nothing twice; one on another model gets its own minutes as well.

### It is observational, and it says so

You did not give two models the same work. You gave the expensive one the
problems you expected to be hard. A table that ignores that reports the
expensive model as worse while measuring nothing but your own routing.

Nothing can fix that from a transcript, so two things make it visible instead:
the caveat is printed under every table, and the same figures are broken out per
kind of work — most of "this model is worse" turns out to be "this model was
given the debugging".

One agent that switched models mid-way is credited to whichever cost it the
most. The transcript records which model billed a request, not which model
asked for a given tool call.

## What counts as editing

An edit tool — `Edit`, `Write`, Codex's `apply_patch` — is the obvious case. The
less obvious one is a shell command, and missing it was wrong in a way worth
recording: a session driven in "do the work through Bash" mode edits with
`sed -i`, a heredoc and a redirect and never touches an edit tool at all. cctop
counted no edits, filed those sessions as Testing, and then reported them as
having spent money and changed nothing.

So a shell command is read for a write: an in-place editor, `tee`, a copy or a
move, or a redirect whose target names a path rather than a descriptor —
`2>&1`, `>&2` and `/dev/null` being the three that appear constantly and write
nothing worth counting.

This undercounts, deliberately. A `python3 - <<PY` whose script calls
`open(p, "w")` writes a file that the command line cannot reveal. A missed write
leaves a session looking quieter than it was; a false one would accuse somebody
of editing a file they only read, and that is the worse mistake.

One consequence to know about: a shell write carries no file name, so it counts
toward *whether* a session changed anything but not toward the one-shot rate,
which needs a path. `1-shot`, `reworked` and `$/file` therefore describe
edit-tool work only.

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
