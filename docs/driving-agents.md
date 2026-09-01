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
| inside rmux | `rmux send-keys` into the pane holding the agent | rmux |
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
any other way still show up in cctop; they just can't be typed into unless rmux
or the root path applies.

## Resuming a session

`R` reopens the selected session in a tab of its own, running the harness's own
resume command — `claude --resume <id>`, `codex resume <id>`,
`opencode --session <id>`, `pi --session <id>` — in the directory the session
was working in. This is the way into a session cctop did not start: `a` can only
show the terminal of an agent cctop is already hosting, while resuming starts a
fresh agent from the transcript and so works for any session in the table,
however it was launched and however long ago it ended.

The tab is named after the session, not the command that reopened it — `claude ·
Improve super cctop` rather than `claude --resume 4ebf1ab4-2ef8-4fb2-a7d5-…`,
whose only variable part is a uuid nobody reads.

Cursor, Gemini and Windsurf keep their conversations inside an editor and have
no such command, so `R` says so rather than guessing at a flag; `y` copies their
transcript path instead. Resuming a session that is *still running* asks first —
two agents appending to one transcript is not something the harnesses
coordinate.

## Tabs and splits

The session table is tab 1. `t` opens another: pick an agent — whichever of
`claude`, `codex`, `opencode`, and `pi` you have installed — or your shell, and
it starts on a pty cctop owns and draws in the window.

The launcher says where the agent will start, and `c` makes that line editable:
type or paste a path, `~` included, `Enter` to take it and `Esc` to keep the one
it had. An empty field means the directory cctop itself was started in. A path
that does not name a directory is refused in the field rather than at launch —
by then the launcher is gone, and the failure arrives from inside the pty as a
message about spawning.

You do not have to remember the path. The field lists what it can see under
itself: with nothing typed, the projects cctop has seen agents run in, newest
first; type part of a name and the list narrows by substring, so `api` finds
`~/work/api` without the `~/work`. Once the text reads as a path — anything with
a `/` in it — the list comes off the disk instead: the directories under it,
hidden ones only once you type the dot. `Tab` fills in as far as the entries
agree (one match completes outright and adds the `/`, so you can walk down a
tree a keystroke at a time), `↑`/`↓` move into the list and back out into the
text, and `Enter` on a highlighted entry takes it. Clicking works too: once to
pick, again to take it. Reattaching to a running agent does not offer this: that
agent is already somewhere, and a path typed for it would be ignored. `Alt+v` and `Alt+s` split
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

When [rmux](https://github.com/Helvesec/rmux) is installed, every tab's agent
runs inside an rmux session of its own and what cctop hosts is only the rmux
client. Quitting cctop detaches — the agent does not notice and carries on. On
the way out cctop says how many it left behind.

Opening cctop again restores those rmux-backed tabs automatically, with their
scrollback intact. Closing a pane is the other thing entirely: `Alt+w` ends the
agent, because a window you closed should stay closed rather than come back at
the next launch. The launcher (`t`) still lists any running agents that are
not already open, so you can attach to them on demand. `R` on a session's row
does the same thing by another route — a resumed session's rmux session is named
after it, so pressing `R` twice reattaches rather than starting a rival agent on
one transcript.

Each of those agents is listed by what the dashboard calls it, the directory it
is working in, and what it last reported through its hooks — `asking`, `working`,
`idle` — so which one to go back to is a decision rather than a guess. A ringed
dot (`◉`) means something is already attached to it, in another terminal or
another cctop; attaching a second client works, but the two then share one
window's size.

This is the same hook feed the dashboard uses, and it reaches these agents for
the same reason: cctop looks up the process *inside* the rmux session rather than
the client in front of it. So a tab whose agent is in rmux still blinks when that
agent asks a question, and still keeps quiet while it is only thinking. `a` on a
session's row uses that lookup in reverse and opens the agent's own terminal,
whether cctop is holding its pty or rmux is.

cctop turns the status bar off in the sessions it creates, and only in those. Its
row is duplicate chrome inside a pane that already has a border and a footer, and
its clock repaints on a timer — which, to anything watching for the screen to
stop changing, is indistinguishable from an agent still at work.

Without rmux installed, none of this applies and tabs behave as they always did:
the agent runs on a pty cctop owns and goes when cctop goes. cctop offers to
install rmux the first time a tab would have used it — `brew install rmux`,
`cargo install rmux --locked`, or rmux.io's install script, whichever this
machine can run — and one "no" holds for the run.

#### Why rmux and not tmux

cctop drove tmux until 0.8, and could be pointed at rmux with `CCTOP_MUX=rmux`.
It now drives rmux only. rmux reimplements tmux's command surface, so everything
above is the same set of commands it always was, and it answers two things tmux
has no answer for: a native Windows backend, and browser sharing.

The cost lands on upgrade, and is worth saying plainly: the two keep separate
daemons and separate sessions. Agents still running under a tmux server are
still running — nothing here kills one — but cctop stops being able to see them.
`tmux attach -t cctop-<provider>-<id>` reaches them, and `tmux ls` lists them.

### Sharing an agent to a browser

**`W` shares the selected agent's terminal to a browser.** cctop runs
`rmux web-share -t <session>` and puts the operator link on your clipboard — open
it on a phone and you are typing into that agent. The pairing code goes on
cctop's status line; the link never does, because it grants input to a live
coding agent and a status line survives into a screenshot.

The link reaches this machine over cctop's own TryCloudflare quick tunnel, the
same kind `cctop serve --tunnel` opens, handed to rmux as `--tunnel-url` rather
than letting it raise a second one through a provider of its own. One way out of
the machine, opened on the first `W` of a run and closed when cctop exits. If
the tunnel cannot be registered the share still happens and the status line says
`this machine only` — a link that works from this browser is worth more than no
link, and a link that silently reaches nothing is worth less than either.

rmux ships six tunnel providers of its own and deliberately not this one: a
`trycloudflare.com` hostname, [its docs say](../rmux/docs/web-share.md), "can
take an unpredictable amount of time to become reachable" and carries no uptime
guarantee — so it points anyone who wants one at `--tunnel-url`, which is the
door cctop comes through. Expect a link to 502 for a moment after it is minted,
and reach for a named tunnel or your own ingress for anything that has to stay
up. rmux also cannot tell viewers apart by source IP once traffic arrives
through a tunnel, so its per-viewer caps are not a control you have here.

What the tunnel carries is not what `serve` puts through one. rmux encrypts the
share end to end and pairs it with a PIN, so Cloudflare moves ciphertext it
cannot read; the served page is ordinary HTTPS that Cloudflare terminates. Both
are still a credential to a live coding agent: share the link the way you would
share a shell, and end it when you are done — `rmux web-share list` shows what is
currently shared and `rmux web-share off` ends all of it.

For the QR code and the read-only spectator link, run
`rmux web-share -t <session>` in a terminal yourself — it draws a card per role
that only renders to a tty. cctop reads only the operator link, and reads it from
stderr because that is the stream rmux puts it on; the spectator link goes to
stdout, and the two are the same shape, so the stream is the only thing telling
them apart.

### What cctop keeps, and what goes to the agent

Panes cctop started are cctop's to end: closing one (`Alt+w`, or the agent
exiting on its own) takes the agent with it, rmux session included. Quitting
cctop is the opposite and leaves them running. A pane opened onto someone
else's session with `a` only stops watching.

Everything cctop does not claim goes to the focused agent, `Ctrl-C` included:
inside a pane, that interrupts the agent rather than quitting cctop. The
function keys and `Alt` are what cctop keeps — the function keys because they
are the ones agents never want, and `Alt` because `Ctrl` is the agent's. No
function key is passed on: `F10` quits and `F5` refreshes where you pressed
them, `F12` goes back to the dashboard, and the rest bring the dashboard
forward and act there. The full list is in
[Reading the table](the-table.md#every-key).

### Shift+Enter, and the keys a terminal cannot spell

Enter is a carriage return, and Shift+Enter is the same carriage return: the
difference exists only in a protocol invented to carry it. So it has to be
asked for at both ends. cctop asks its own terminal for the disambiguating
form on the way in, where the terminal says it can send one, and sends
Shift+Enter on to the agent as `CSI 13;2u` — but only to an agent that turned
a keyboard protocol on, or through rmux, which rewrites it to a plain Enter
when the pane's program did not. A shell in a tab is never handed a sequence it
would print as text.

Which agents ask, as of writing: Claude Code turns on both the kitty protocol
and xterm's `modifyOtherKeys`, and Codex turns on kitty and explicitly turns
`modifyOtherKeys` off. If your terminal has no extended keyboard protocol at
all, nothing changes: `Ctrl+J` is the newline every agent also accepts.

### The questions an agent asks the terminal

An agent asks the terminal what it can do before it draws anything — Codex asks
five questions, Claude Code two. Under `cctop run` the real terminal is behind
cctop and answers for itself. A pane is different: cctop rebuilds the screen
from the agent's output, and a picture of a terminal has nothing to say back,
so a hosted agent's questions used to go into the dark and it assumed the least
about them.

The shim now answers the ones with an honest fixed answer: device attributes,
device status, synchronized output, and the foreground and background colour —
which it reports from cctop's own palette, so an agent set to follow the
terminal's theme follows the pane it is drawn in. The cursor position is
deliberately not answered; the shim relays bytes rather than parsing them, so
it does not know where the cursor is, and a made-up position would put an
agent's first frame in the wrong place.

## Getting pinged when a session needs you

cctop is a monitor you look away from, so `w` turns on the other direction:
when a session that was working starts waiting for your input — or the agent
exits — cctop rings the terminal bell and raises a desktop notification. The
setting is off by default and remembered between runs.

Both are the terminal's own mechanisms, so nothing has to be installed. `BEL`
is what rmux turns into a window's bell flag; the desktop notification
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

### When the agent is the one ringing

That is cctop's own bell, rung about a session's state. An agent can also ring
for itself — Claude Code with `preferredNotifChannel: terminal_bell` does
nothing else, and several harnesses send an `OSC 9` notification — and a tab
whose agent rang turns the attention colour immediately, ahead of every
inference the tab bar otherwise makes. If the notification carried text
("Claude needs your permission to use Bash"), the status line shows it once,
when you switch to the tab.

This matters because it is the only signal that arrives *in time*. Everything
else the tab bar has is inferred: a hook event on a six-second timer, or a
screen that stopped moving. An agent blocked on a permission prompt keeps its
spinner turning and reports itself as working, so it used to be drawn as busy
for as long as it sat there.

The bell survives the whole stack — the agent rings, rmux passes it to its
client, the shim relays it, and the pane's parser keeps it instead of parsing it
away. cctop does not pass it on to your own terminal: the bell is answered by
looking at the pane, and a beep per agent per prompt is the alarm clock this
page already argues against.

## Handing a session to a different agent

`O` takes the selected session's context across to another harness. Where `R`
puts the *same* agent back on the *same* transcript, a handoff carries what the
session was doing over to a different agent entirely — the one thing no harness
can do for itself, since each can only read its own transcripts.

cctop writes a markdown brief, opens the launcher, and starts whichever agent you
pick with a line pointing it at the file. Where the harness takes an opening
prompt on its command line — `claude`, `codex`, `opencode` — that line is part of
the argv, so the agent opens on the brief. Anything else is typed at once it has
had a moment to start listening.

The distinction matters more than it looks. Every one of these CLIs asks the
terminal what it can do as it starts, and reads its own input looking for the
answer; a line arriving in that window loses however much of itself was in the
queue at the time. Handing a Claude session to Codex used to produce a prompt
beginning halfway through the path, so Codex went looking for a file that had
never existed and asked for the brief to be pasted instead. An argument cannot
be eaten that way.

The brief holds the task, the plan the
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

### Claude to Claude, the conversation goes whole

The brief exists because no harness can read another's transcripts. Between two
Claudes that limit is not there, so handing a Claude session to `claude` copies
the transcript instead: the file is written into the receiving account's
`projects/` directory under a fresh session id, and the new agent is started with
`claude --resume` on the copy. It opens knowing everything the first one knew,
not everything a summary could carry.

It is a copy, not a second agent on one transcript. The two diverge from the
moment the fork is taken, and the session that was handed over is left exactly as
it was found — still listed, still resumable, still the only writer of its own
file. That is also what makes this different from `R`, which refuses to put a
second agent on a transcript for precisely that reason.

The cost is the one the brief was written to avoid: the whole window, tool output
and all, replayed into a fresh one. That is the trade being made on purpose —
everything is carried because everything can be. Hand the same session to any
other agent and it gets the brief, which is the only form that agent can read.

The same brief is available without the UI:

```bash
cctop --handoff            # the most recently active session, as markdown
cctop --handoff 2abd15fe   # a session id, or any unique prefix of one
```
