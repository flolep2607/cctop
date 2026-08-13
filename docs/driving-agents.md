# Driving agents from cctop

[← back to the README](../README.md)

cctop starts as a viewer, but it can also answer a waiting agent, reopen an old
session, and hold several agents on one screen. Everything here is optional —
the table works without any of it.

## Typing into a session

`s` opens a one-line prompt (prefilled with `continue`) and types it into the
terminal driving the selected agent, as if you had typed it there — useful for
the sessions whose status dot has gone yellow or red waiting on you.

An agent reads its keyboard from a pty, and only whoever holds that pty's master
side can put bytes into it — normally the terminal emulator, which offers no way
in. (Writing to `/proc/<pid>/fd/0` or `/dev/pts/N` reaches the *output* side and
just paints the screen; it does not reach the agent.) So cctop needs one of these
to be true, and tries them in this order:

| The session runs… | How | Requirements |
|---|---|---|
| under `cctop run <agent>` | cctop owns the pty and typing goes through a unix socket | none; verified on Linux, unverified on macOS |
| inside tmux | `tmux send-keys` into the pane holding the agent | tmux |
| in a plain terminal | `TIOCSTI` pushes bytes into the tty's input queue | Linux, and cctop as root — `CAP_SYS_ADMIN` clears both of the kernel's gates. Without root it also needs `sysctl -w dev.tty.legacy_tiocsti=1` (off by default since 6.2) *and* cctop sharing the agent's controlling terminal, which in practice it doesn't |

The first is the one worth adopting — no root, no multiplexer, and the session
looks and behaves exactly like one started directly. cctop sets it up for you: on
its first interactive run it aliases `claude`, `codex`, `opencode`, and `pi` to
`cctop <agent>` — a marked block appended to `~/.zshrc` and `~/.bashrc`, and
`~/.config/fish/conf.d/cctop.fish` for fish (each only where that shell is
already configured; the fish file is unverified, having been written on a machine
without fish).

```bash
cctop --remove-alias     # take it out again; deleting the block by hand also works
cctop --install-alias    # put it back
```

Every alias is guarded by a command-exists test, so it defines nothing
unless both cctop and the agent are installed — uninstall cctop and `claude`
goes back to meaning `claude`. The block is written once: remove it and cctop
won't add it again.

The `run` is optional: any first argument that names an executable is launched
this way, so `cctop claude --dangerously-skip-permissions` works and the flags
reach the agent rather than cctop. A word that isn't a command still gets cctop's
usage error, so a typo doesn't silently try to run something.

`cctop run` proxies your terminal byte-for-byte (including resizes) and exits
with the agent's own exit code, so it is a transparent stand-in. Sessions started
any other way still show up in cctop; they just can't be typed into unless tmux
or the root path applies.

## Resuming a session

`R` reopens the selected session in a tab of its own, running the harness's own
resume command — `claude --resume <id>`, `codex resume <id>`,
`opencode --session <id>`, `pi --session <id>` — in the directory the session
was working in. This is the way into a session cctop did not start: `a` can only
show the terminal of an agent cctop is already hosting, while resuming starts a
fresh agent from the transcript and so works for any session in the table,
however it was launched and however long ago it ended.

Cursor, Gemini and Windsurf keep their conversations inside an editor and have
no such command, so `R` says so rather than guessing at a flag; `y` copies their
transcript path instead. Resuming a session that is *still running* asks first —
two agents appending to one transcript is not something the harnesses
coordinate.

## Tabs and splits

The session table is tab 1. `t` opens another: pick an agent — whichever of
`claude`, `codex`, `opencode`, and `pi` you have installed — or your shell, and
it starts on a pty cctop owns and draws in the window. `Alt+v` and `Alt+s` split
the tab you are in, side by side or stacked, so `claude` and a shell for
`git diff` are one keystroke apart. Fresh tabs start in the directory where you
started `cctop`; use `R` to reopen a session in that session's project directory.
Agent tabs also show their subscription-window usage and time until reset when
that provider reports it.

Each pane is resized to the rectangle it is given rather than cropped to it, so
a split is two agents each drawing a real screen — not two crops of one. The
Overview stays above the panes, so cost and alerts remain visible while you
answer an agent.

`a` does the same for a session already running: it puts that agent's own
terminal in a tab, live — its spinner, its permission prompts, whatever it is
drawing. That needs cctop to hold the pty, so it works for sessions started with
`cctop <agent>` and no others; the aliases above make that every session you
start from a shell. On attaching, the shim replays its recent output so the
screen is rebuilt immediately rather than at the agent's next repaint.

A tab you are not looking at is marked **green** when its agent has stopped
drawing — its turn is over and the prompt is yours — and blinks **amber** only
when it has explicitly asked a question and is blocked on the answer. Amber wins
when a tab has both. The tab you are on never blinks, since its focused pane is
already in front of you.

The two are measured differently, because only one of them is in the transcript.
The question is: an agent that calls an ask-the-user tool records it. "Finished
its turn" is not — in a transcript, answering you and still thinking look the
same. So idleness is read off the pane's own screen instead: a working agent
repaints constantly (a spinner's elapsed counter alone ticks every second), so
two seconds of a still screen means the agent is waiting on you. That needs no
per-harness parsing and works for anything you open in a tab, shells included.

### Tabs outlive cctop

When tmux is installed, every tab's agent runs inside a tmux session of its own
and what cctop hosts is only the tmux client. Quitting cctop detaches — the
agent does not notice and carries on. On the way out cctop says how many it left
behind.

Opening cctop again restores those tmux-backed tabs automatically, with their
scrollback intact. Closing a pane is the other thing entirely: `Alt+w` ends the
agent, because a window you closed should stay closed rather than come back at
the next launch. The launcher (`t`) still lists any running agents that are
not already open, so you can attach to them on demand. `R` on a session's row
does the same thing by another route — a resumed session's tmux session is named after it, so
pressing `R` twice reattaches rather than starting a rival agent on one
transcript.

Each of those agents is listed by what the dashboard calls it, the directory it
is working in, and what it last reported through its hooks — `asking`, `working`,
`idle` — so which one to go back to is a decision rather than a guess. A ringed
dot (`◉`) means something is already attached to it, in another terminal or
another cctop; attaching a second client works, but the two then share one
window's size.

This is the same hook feed the dashboard uses, and it reaches these agents for
the same reason: cctop looks up the process *inside* the tmux session rather than
the client in front of it. So a tab whose agent is in tmux still blinks when that
agent asks a question, and still keeps quiet while it is only thinking. `a` on a
session's row uses that lookup in reverse and opens the agent's own terminal,
whether cctop is holding its pty or tmux is.

cctop turns the status bar off in the sessions it creates, and only in those. Its
row is duplicate chrome inside a pane that already has a border and a footer, and
its clock repaints on a timer — which, to anything watching for the screen to
stop changing, is indistinguishable from an agent still at work.

Without tmux installed, none of this applies and tabs behave as they always did:
the agent runs on a pty cctop owns and goes when cctop goes. The fallback is
silent — tmux is how this is better, not how it works.

### What cctop keeps, and what goes to the agent

Panes cctop started are cctop's to end: closing one (`Alt+w`, or the agent
exiting on its own) takes the agent with it, tmux session included. Quitting
cctop is the opposite and leaves them running. A pane opened onto someone
else's session with `a` only stops watching.

Everything cctop does not claim goes to the focused agent, `Ctrl-C` included:
inside a pane, that interrupts the agent rather than quitting cctop. `F12` and
`Alt` are what cctop keeps — the function keys because they are the ones agents
never want, and `Alt` because `Ctrl` is the agent's. The full list is in
[Reading the table](the-table.md#every-key).

## Getting pinged when a session needs you

cctop is a monitor you look away from, so `w` turns on the other direction:
when a session that was working starts waiting for your input — or the agent
exits — cctop rings the terminal bell and raises a desktop notification. The
setting is off by default and remembered between runs.

Both are the terminal's own mechanisms, so nothing has to be installed. `BEL`
is what tmux turns into a `monitor-bell` window flag; the desktop notification
is OSC 9, which iTerm2, Ghostty, kitty, WezTerm and Windows Terminal raise as a
real notification and everything else quietly ignores.

It rings on the *crossing*, never on the state: a session that is waiting for
you is still waiting on the next refresh, and ringing for that would be an
alarm clock. For the same reason, turning notifications on doesn't ring for
every session that happens to be idle at that moment — cctop tracks the states
whether or not the bell is on, and starts from what it already knows.

The session that rang keeps a bell marker (`◉`) on its row for 30 seconds, and
the footer keeps naming it (`Bell: ◉ cctop · waiting for input · 12s ago`)
until you select it. `b` jumps straight there. A bell out of a dozen panes is
never a mystery, even if you were away when it rang.

One thing it deliberately does *not* ring for: an agent that has simply
finished its turn and is sitting at its prompt. In the transcript that looks
the same as an agent still thinking, and a timer would fire in the middle of
every long reasoning turn.

## Handing a session to a different agent

`O` takes the selected session's context across to another harness. Where `R`
puts the *same* agent back on the *same* transcript, a handoff carries what the
session was doing over to a different agent entirely — the one thing no harness
can do for itself, since each can only read its own transcripts.

cctop writes a markdown brief, opens the launcher, and types a line at whichever
agent you pick pointing it at the file. The brief holds the task, the plan the
session was working to, the files it changed and read, the commands it ran, what
it delegated, and what it looked up — with paths relative to the project, and
every list bounded so a long session hands over its most recent and most-touched
entries rather than all of them.

It is deliberately not a transcript. Replaying a conversation into a fresh
window spends the context it is supposed to save, and most of what it spends it
on — tool output, file contents the new agent can read itself — is what the
receiving agent should gather first-hand anyway. What does not survive a restart
is the intent, and that is what gets carried.

Because it is built from the normalised session data rather than from any one
transcript format, it works from and to all seven harnesses.

The same brief is available without the UI:

```bash
cctop --handoff            # the most recently active session, as markdown
cctop --handoff 2abd15fe   # a session id, or any unique prefix of one
```
