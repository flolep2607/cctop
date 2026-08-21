# Integrations

[← back to the README](../README.md)

Three ways cctop connects to something outside itself: letting the agents
report their own state, letting agents query cctop, and reading another
machine.

## Letting the agents tell cctop directly

Both of those are cctop guessing. The agents can just say it:

```bash
cctop --install-hooks           # for this user: every agent below
cctop --install-hooks project   # only for the project in this directory
cctop --remove-hooks            # take it back out; same scopes
cctop --hooks-status            # what is installed, and whether it works
```

Press `h` (or `F8`) in the UI for the same thing with the install and remove
keys on it, so none of this needs you to leave cctop or restart it.

One install covers five agents, each asked in its own dialect:

| Agent | Where it is written | What is written |
|---|---|---|
| **Claude Code** | `~/.claude/settings.json`, or `<project>/.claude/` | twelve hooks — `Stop`, `StopFailure`, `Notification`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PostToolUseFailure`, `SessionStart`, `SessionEnd`, `PreCompact`, `SubagentStop` |
| **Gemini CLI** | `~/.gemini/settings.json`, or `<project>/.gemini/` | eight — `BeforeAgent`, `AfterAgent`, `BeforeTool`, `AfterTool`, `Notification`, `SessionStart`, `SessionEnd`, `PreCompress` |
| **Cursor** | `~/.cursor/hooks.json`, or `<project>/.cursor/` | seven — `stop`, `beforeSubmitPrompt`, `beforeShellExecution`, `sessionStart`, `sessionEnd`, `preCompact`, `subagentStop` |
| **Codex** | `~/.codex/hooks.json`, or `<project>/.codex/`, and `~/.codex/config.toml` | ten hooks — `Stop`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `SessionStart`, `SessionEnd`, `PreCompact`, `PostCompact`, `SubagentStop` — and `notify = ["cctop", "hook", "codex"]` |
| **OpenCode** | `~/.config/opencode/plugins/cctop.ts`, or `<project>/.opencode/` | a plugin, since OpenCode extends by code rather than by command |

Codex is asked twice, because its two ways of saying things fail differently.
Its hook framework borrowed Claude Code's spelling wholesale — same event names,
same nested `hooks` object, same JSON on stdin — so cctop needed no new reader
for it, and it reports everything Claude Code does bar the two failure events
Codex has no use for: its `PostToolUse` fires for a command that exited non-zero
as well as one that succeeded, and a turn that dies has no second ending. But a
Codex hook is inert until a person has looked at it: run `/hooks` inside Codex
and trust them, or they deliver nothing. `notify` stays installed alongside for
exactly that reason. It says only that a turn finished, and it says it the
moment it is written.

Nothing on disk says whether you have trusted them — Codex records that against
a hash of the hook, somewhere it does not document — so cctop reads it from the
events instead. `notify` carries no permission mode and a hook carries one on
every event worth having, which makes a Codex row that knows how much it asks
before it acts a Codex whose hooks are firing. Until one has, the panel keeps
the reminder on Codex's line, and starting a Codex from the launcher says it
where it is a keystroke away rather than a thing to remember:

```
Started codex in ~/proj — run /hooks in it and trust cctop's to see it here
```

A Codex you reattached to is left out of that: it has been running since before
whatever is installed now, so the answer for it is a restart rather than a
keystroke.

Each hook runs `cctop hook <event>`; the plugin and Codex's `notify` hand the
event over as an argument instead. Whichever way it arrives, it is reduced to
the same three facts — which session, what happened, which directory — and sent
to every running cctop over a unix socket. The agents disagree about all three
spellings (`session_id`, `thread-id`, `conversation_id`, `sessionID`; a `cwd` or
a `workspace_roots` array), so each is read under every name anyone uses.

Half the list is there to *end* a state rather than start one, which is the
half that goes wrong when it is missing. Claude Code fires `PostToolUse` only
for a tool call that succeeded and `Stop` only for a turn that ended cleanly, so
without their partners — `PostToolUseFailure` and `StopFailure` — a grep that
matched nothing leaves a tool call in flight forever, and cctop draws a tool
call in flight over a still screen as a permission prompt waiting for you.
`PermissionRequest` is the same fact said properly: it fires the instant Claude
Code asks, where the `Notification` for the same prompt is on a six-second
timer and the tool-in-flight reading is a guess.

If you installed before an event was added — `PostToolUseFailure`, or a whole
agent — that install is short exactly those events, and cctop fills it in on
the way up rather than only mentioning it in the panel. It is filled in *where
it stands*: an install naming a different cctop keeps that binary, because
events reach every cctop that is listening whoever fires them. Only an install
naming a binary that no longer exists is repointed at this one, there being
nothing left to respect. The OpenCode plugin is topped up the same way, by
being rewritten: cctop owns that file outright, and an old copy of it forwards
fewer events for the same reason an old settings file registers fewer hooks.

Cursor also reads Claude Code's `settings.json` of its own accord, so with both
installed each moment arrives twice; that costs a process spawn and nothing
else, since the second event says exactly what the first did. The same goes for
Codex once its hooks are trusted: `Stop` and `notify` both report the end of a
turn, and applying the same fact twice changes nothing.

A state cctop was told about is believed until the agent says otherwise, with
one exception: "I am working" has a shelf life of fifteen minutes. An agent
mid-turn says something again well inside that — the next tool starts, the tool
comes back, the turn ends — so silence that long means the event that would
have closed it never came, which is what a killed session, a closed terminal
and an interrupted turn all look like. "I am waiting on you" never expires,
because nothing else was going to say it again and you may be away for the
afternoon.

A reported turn beats a still screen: the green appears the instant the turn
ends rather than two seconds later, and the amber no longer waits for a
transcript to be written. `SessionStart` and `SessionEnd` do the same for the
table itself — a session you just started appears at once instead of at the next
poll, and one that has exited stops claiming a state it is no longer in.
Sessions already running keep their old hooks until they restart.

The installer merges into your settings rather than writing them, recognises its
own entries by their shape so it is idempotent and removable however the binary
is named, writes through a temporary file so an interrupted write cannot leave
you with no settings, and refuses outright if the file is not valid JSON — or,
for Codex, not valid TOML — rather than replacing it. Codex's config is edited
in place, so the comments and layout around it survive. Codex allows only one
`notify` program, so an entry that is not cctop's is reported and left alone.
Agents are independent of each other: a `notify` slot already spoken for, or a
settings file that will not parse, is one line of the report and does not undo
the installs that worked.

The OpenCode plugin is the one integration that runs *inside* an agent rather
than beside it, so it is the one place the exit-code guarantee below cannot
help. Its handler ignores everything it does not report, wraps the rest in a
`try`, and never waits for the process it starts. cctop writes that file whole
and deletes it whole — it is the only file here it owns outright.

`--hooks-status` and the `h` panel exist because none of this is visible
otherwise: an install that points at a cctop you have since moved or deleted
looks exactly like an agent with nothing to say. cctop repoints that one for you
on startup — there is no behaviour to preserve in a command that runs nothing —
but leaves an install pointing at a *different* cctop that does exist alone,
since that is a second install and not a fault.

**`cctop hook` cannot break your session.** An agent reads a hook's exit code as
a decision — exit 2 blocks the tool call — so this one exits 0 unconditionally,
writes nothing to stdout, and is bounded by a 250ms deadline covering the whole
exchange, on a thread the process abandons if it overruns. No cctop running, a
stale socket, malformed input, a wedged cctop, an outright panic: every one of
them is a silent, prompt success. Dropping an event is always cheaper than
stalling an agent.

## Letting agents see each other

```bash
cctop --mcp
```

serves the Model Context Protocol on stdin/stdout, so an agent can ask cctop
what the *other* agents on the machine are doing — the thing none of them can
find out for themselves. Point a harness at it the way you would any stdio MCP
server:

```json
{"mcpServers": {"cctop": {"command": "cctop", "args": ["--mcp"]}}}
```

Four tools, all read-only:

- **`list_sessions`** — every session, any harness: model, directory, branch,
  tokens, estimated cost, context occupancy, and whether it is still running.
  Filterable by `running_only` and by `directory`, which is how an agent asks
  who else is in this repo.
- **`check_conflicts`** — the `!` column, asked rather than read: give it a
  directory and the files you are about to change, and it answers with the
  running agents in the same repository and which of those files they have
  already written. The one question an agent cannot answer for itself and pays
  for getting wrong, since a lost edit arrives with no error attached.
- **`get_session_context`** — the same brief `O` writes, for one session.
- **`search_sessions`** — the full text of every transcript on the machine,
  with a snippet of each match. Where something was already discussed or
  attempted, in any harness.

Nothing here starts, stops, or types at anything. An agent that can *drive*
other agents is a much larger proposition than one that can *see* them, and the
visibility is the half with no downside.

## More than one machine

cctop is otherwise a monitor of one computer, which is the wrong shape if the
agents are not all on it — a laptop in front of you and a devbox doing the heavy work,
each with its own idea of what today cost.

```bash
cctop --host devbox --host flo@builder
CCTOP_HOSTS=devbox,flo@builder cctop     # the same, from the environment
```

Their sessions join the table with a **HOST** column saying where each one is,
and their spend joins every total and every window in the Overview. The column
appears only when a host is configured; on one machine it would be the word
`local` repeated down the screen.

The mechanism is the dullest one available: a thread per host runs
`ssh <host> cctop --json` every 15 seconds and reads what comes back. No daemon,
no port, no protocol of cctop's own, nothing to install on the far side beyond
the cctop that is already there — ssh has the authentication and the transport,
and `--json` is the wire format whether or not anyone sends it over a wire. A
host must therefore have cctop installed and your key must reach it without a
passphrase prompt (`BatchMode=yes`, so a host that would ask is an error rather
than a hang).

**If it says `command not found`,** that is the usual first result and it is not
a lie: `ssh host cctop` runs a *non-interactive* shell, which on most setups
skips the rc file that put `~/.local/bin` or a version manager's shim on `PATH`.
Name the binary instead:

```bash
cctop --host devbox:/usr/local/bin/cctop
```

Remote rows are **read-only**. `d`, `k`, `s`, `a`, `R` and `O` all refuse them by
name, because every one of them reaches into *this* machine — a signal to a
process, a transcript on disk, a pty — and the same path on this filesystem is a
different file. For the same reason the branch shown is the one that machine
read, not whatever happens to sit at that path here, and the `!` conflict column
carries the verdict that machine reached: each one detects its own overlaps,
being the only one that can see its own disk.

Only the Info panel is filled in for a remote session. Cost, Context, Tool
Activity and the rest are readings of a transcript that stays where it is, and
they say so rather than drawing zeroes.

A host that stops answering keeps its last rows and says so in the footer
(`⚠ devbox: Permission denied`). Blanking them would be the stronger claim —
those agents have not stopped, cctop has merely lost sight of them.

## Every user on the machine

Run cctop as root and it reads every user's sessions, not root's own — which on
most machines is an empty table, since nobody runs an agent as root. This is
what `htop` does with processes, for the same reason: the point of running a
monitor privileged is to see the machine rather than your corner of it.

```bash
sudo cctop                       # every user's sessions
sudo CCTOP_ALL_USERS=0 cctop     # root's own only, the way any other user sees it
CCTOP_ALL_USERS=1 cctop          # sweep without root, where the homes are readable anyway
CCTOP_HOMES=/export/people/ana:/export/people/bo cctop   # homes discovery cannot find
```

Homes come from `/etc/passwd` — root and the login accounts, service accounts
skipped — plus whatever sits under `/home` (or `/Users`), which catches users
served by LDAP or SSSD rather than the local file. `$CCTOP_HOMES` names any the
machine keeps somewhere else entirely, `:`-separated as a `PATH` is.

Rows gain a **USER** column naming whose session each one is, blank for your own
and hidden entirely when only your own homes are in view. The name is also
searchable, so `/ana` filters the table to that person's sessions.

What cctop *does* stays privileged in the ordinary way: as root, `k` really will
kill someone else's agent and `d` really will delete their transcript. The one
thing it declines to guess is identity — the Account line and the `account`
field in `--json` are read from your own credentials, so they are left off
another user's row rather than stamped with your email.
