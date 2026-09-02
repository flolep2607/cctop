# `cctop optimize` and `cctop compare`

**Status:** design note, nothing built.
**Accepted:** 2026-09-02, from ideas #1 and #4 in [codeburn.md](codeburn.md).
Idea #5 (task classification) turns out to be a prerequisite for both and is
folded in here. Idea #2 (auto-apply with undo) is deliberately left out of the
first version — see the last section.

---

## Why these two and not the rest

cctop already puts the raw material on screen. `ERR%` is the share of a
session's tool calls that failed. The context panel says where the window went,
including an Unaccounted bar. Tool Activity says what was called. Compaction
cadence catches a session rebuilding a window it keeps refilling.

What none of it says is *what to do about it*. A number that is bad tells you
something is wrong; it does not tell you which of your habits caused it. These
two commands are the same data phrased as a sentence.

They also point in opposite directions, which is why they are separate commands
rather than one:

- **`optimize`** looks at *you* — your configuration and your habits, across
  every session.
- **`compare`** looks at *the models* — how each one behaves on the work you
  actually give it.

## The prerequisite: what kind of work was this?

Both commands need to know what a session was *doing*, because every interesting
metric is meaningless in aggregate. A 30% one-shot rate is alarming for editing
and expected for debugging. "Half your spend went to conversation" is only
sayable if conversation is a category.

Codeburn's thirteen categories are deterministic — no model call, just tool
names and prompt keywords — and cctop has the tool history to do the same:

| Category | Signal |
|---|---|
| Coding | Edit, Write |
| Debugging | error and fix keywords alongside tool use |
| Feature work | "add", "create", "implement" |
| Refactoring | "refactor", "rename", "simplify" |
| Testing | test runners in Bash |
| Exploration | Read, Grep, search with no edits |
| Planning | plan mode, task creation |
| Delegation | subagent spawns |
| Git | commit, push, merge |
| Build and deploy | build tools, docker, package managers |
| Brainstorming | "what if", "design", "brainstorm" |
| Conversation | no tools at all |
| General | everything else |

Two things to get right that codeburn's docs do not discuss. Keyword rules are
English-shaped and will misfile a French or Chinese prompt — the tool-based
signals are the reliable ones and the keyword ones should only break ties.
And a session is not one category: it is a *sequence*, and the honest unit is
the turn, with the session reported as a distribution. That also makes the
"half your budget went to conversation" claim precise rather than rhetorical.

This is cheap, deterministic, and it is a new column in the table on its own
merits, independent of either command.

## `cctop optimize`

Findings, ranked by what they cost, in three classes:

- **fix** — correctable without a judgement call
- **habit** — only the user can change it
- **note** — informational, no action implied

Each finding carries a token and dollar estimate, a copy-pasteable remediation,
and a label saying whether the saving was **measured** (counted from real token
figures) or **estimated** (modelled). That distinction is the one thing that
keeps the numbers honest and it is already how the cost page is written.

**Detectors cctop can support today**, from data it already parses:

| Detector | Class | Where the data is |
|---|---|---|
| The same file read again across sessions | habit | tool history, file arguments |
| Reads into junk — `node_modules`, `.git`, build output | fix | tool history |
| Low Read-to-Edit ratio | habit | tool history |
| Failing tool calls, repeated | habit | already the `ERR%` column |
| Compaction churn — a window rebuilt and refilled | habit | context series, already charted |
| A bloated `CLAUDE.md` or agent file | fix | read the file, size it against the window |
| MCP servers configured but never called | fix | config, against tool history |
| Agents, skills and commands defined but never invoked | fix | config, against tool history |
| Context dominated by one source | note | the context breakdown panel |
| Sessions that spent and shipped nothing | note | spend against edits |

The last one is the interesting one, and it is where idea #3 (`yield`) leaks in:
cctop knows a session's working directory and which files it edited, so a
session that cost money and touched no file is detectable without any git
correlation at all. That is a weaker claim than "the commits were reverted" and
a much cheaper one.

**Health grade.** Codeburn puts an A–F on it. Worth having, worth being careful
with: a grade is a strong claim and it should be weighted against observed
waste, not against how many detectors happened to fire. A user with three
findings worth 2% of spend has not earned a D.

**What it must not do.** Not scold. The register of these findings should be the
register of the rest of cctop — factual, specific, and prepared to say "this is
probably fine". Codeburn's `keep` class exists for exactly this and should
survive the port.

## `cctop compare`

The same models, side by side, on the work you actually give them.

**Efficiency**, all available from token counts cctop already has: cost per
call, **cost per edit**, output tokens per call, cache hit rate.

**Behaviour**, the more interesting half:

- **One-shot rate**, file-aware. This is the metric worth taking. A retry is
  `Edit foo.rs → something → Edit foo.rs` — the *same* file edited again after
  an intervening call. Editing a different file is not a retry, it is progress.
  That distinction is what makes it a sharper signal than `ERR%`: a failed call
  is noise, an agent editing one file four times is a story. cctop reads Claude
  and Codex at file-level fidelity, which is what this needs; the harnesses that
  do not record file arguments fall back to tool names and should be labelled as
  the weaker measure rather than mixed in silently.
- **Self-correction turns** — how many turns after a failure before forward
  progress resumes.
- **Delegation rate** — share of turns that spawn a subagent.
- **Tools per turn.**
- **One-shot rate per category**, which is the only form of it that means
  anything.

Reported per model, and per model *per category*, because "this model is worse"
is almost always "this model is worse at one kind of work".

**The trap:** this is observational, not an experiment. You did not give both
models the same work — you gave the expensive one the hard problems. A naive
table will say the expensive model has a worse one-shot rate and it will be
measuring your routing, not the model. The command has to say so, on the output,
every time. Segmenting by category helps and does not fix it.

## Surfaces

`o` and `c` in the TUI, matching codeburn's keys, since both are subviews of
data already on screen. Both as commands for scripting, both with `--json`. And
both belong in the browser report — the optimize findings especially, because a
copy-pasteable remediation is more useful somewhere you can actually copy it.

## What is deliberately not in v1

**Auto-apply, undo, and the savings report** (idea #2). The mechanism is good —
`act undo` refusing when the file changed since the apply is a nice touch, and
grading an applied fix against later usage is the only thing that makes an
estimate falsifiable. But writing to somebody's `~/.claude/` config is a
different kind of commitment from reading it, and it should not arrive in the
same release as the detectors that decide what to write. Ship the findings with
their remediations first, find out which ones are right, then automate the ones
that survive.

**The interpretation table** from the end of codeburn.md is a separate, smaller
piece of work and does not depend on either command. It is probably the cheapest
thing in this whole folder and should not wait behind them.
