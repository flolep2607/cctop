# cctop

An htop-like monitor for AI coding agent sessions. Tracks Claude Code, Codex,
Cursor, Gemini CLI, OpenCode, Pi, and Windsurf sessions on your machine: cost
estimation, token usage, tool invocations, subagents, and OS-level process
metrics — refreshed live.

For active sessions, the **HARNESS** column distinguishes the host application
(for example, Cursor) from the model. Unknown launchers remain `─` rather than
being guessed. **BRANCH** is the branch checked out in the session's working
directory, read from the repository's `HEAD` — `@<commit>` when it is detached,
and `─` when the directory is not in a repository at all.

**PERM** is how much a session asks before it acts: `ask`, `edits` (writes files
unasked), `plan` (cannot act at all), or a red `BYPASS` for one started with
`--dangerously-skip-permissions`. Read from the transcript, and kept current by
the session's own hooks when it has them. `─` means the harness does not record
it — today only Claude Code does.

A Rust rewrite of an earlier Node implementation.

## Install

### Download a binary

Grab the archive for your platform from the
[latest release](https://github.com/flolep2607/cctop/releases/latest) and put
`cctop` somewhere on your `PATH`:

```bash
# Linux x86_64 (static — works on any distro)
curl -fsSL https://github.com/flolep2607/cctop/releases/latest/download/cctop-x86_64-unknown-linux-musl.tar.gz | tar xz
sudo install -m755 cctop /usr/local/bin/cctop
```

```bash
# macOS (Apple silicon; use x86_64-apple-darwin on Intel)
curl -fsSL https://github.com/flolep2607/cctop/releases/latest/download/cctop-aarch64-apple-darwin.tar.gz | tar xz
sudo install -m755 cctop /usr/local/bin/cctop
```

On Windows, download `cctop-x86_64-pc-windows-msvc.zip` and extract `cctop.exe`.

Every archive ships with a `.sha256` file next to it:

```bash
curl -fsSLO https://github.com/flolep2607/cctop/releases/latest/download/cctop-x86_64-unknown-linux-musl.tar.gz.sha256
sha256sum -c cctop-x86_64-unknown-linux-musl.tar.gz.sha256
```

macOS will quarantine an unsigned download. If Gatekeeper blocks it:

```bash
xattr -d com.apple.quarantine /usr/local/bin/cctop
```

### Staying up to date

A downloaded binary has no package manager behind it, so `cctop --update`
fetches the newest release for your platform and replaces the running
executable in place:

```bash
cctop --update
```

Replacing the binary needs write access to the directory it lives in. The install
above puts it in `/usr/local/bin` with `sudo`, so updating it needs `sudo` too:

```bash
sudo cctop --update
```

cctop checks for a new release once an hour in the background and, when one
exists, says so in the footer. It never updates itself: the check only reports,
and replacing the binary always takes an explicit `--update`. If you installed
with `cargo install` or a package manager, update it the same way you installed
it — `--update` will refuse rather than overwrite a managed install it cannot
write to.

### With cargo

```bash
cargo install cctop
```

Or straight from the repository, without waiting for a release:

```bash
cargo install --git https://github.com/flolep2607/cctop
```

### From source

```bash
git clone https://github.com/flolep2607/cctop
cd cctop
cargo build --release
```

The binary lands at `target/release/cctop`. It links no system libraries and
needs no runtime — a single file you can copy anywhere.

Building requires Rust 1.88 or newer (the code uses let-chains).

## Usage

```bash
cctop                 # interactive UI
cctop --list          # print a table and exit
cctop --json          # dump full session data as JSON
cctop --plan max      # treat Claude usage as bundled
cctop --delay 5       # refresh every 5 seconds
cctop --clear-cache   # re-extract all session activity; keeps preferences/pricing
cctop --update        # replace this binary with the newest release
cctop run claude      # start an agent on a pty cctop can type into (see Keys)
cctop claude --help   # the same, without the `run`; flags go to the agent
cctop --remove-alias  # remove the shell aliases cctop installs (--install-alias adds them)
```

### Keys

| Key | Action |
|-----|--------|
| `↑`, `↓`/`j` | Move between sessions |
| `PgUp`, `PgDn` | Page up / down |
| `Ctrl+U`, `Ctrl+D` | Half a page up / down |
| `g`, `G` | Jump to first / last |
| `Home`, `End` | Jump to first / last |
| `n`, `N` | Next / previous search match (wraps) |
| `w` | Toggle notifications (see below) |
| `b` | Jump to the session that rang last |
| `←`, `→` | Move between bottom panels |
| `1`–`7` | Jump to a panel directly (`Tab` also reaches Context, the eighth) |
| `Shift+↑`/`↓` | Scroll inside the active panel |
| `f` | Follow mode: keep the selection centered |
| `/` or `F3` | Filter sessions by text (see below) |
| `F6`, `>`, `<` | Sort-by panel |
| `F7` | Filter by age (1d / 1w / 1mo) |
| `#` | Cost floor: only sessions costing ≥ `$X` |
| `` ` `` | Show only running sessions |
| `[`, `]` | Move through the Tool Activity tool filter |
| `v` | Toggle inline diffs for edits |
| `L` | Toggle the Tool Activity live filter |
| `P` / `M` / `T` | Sort by status / memory / cost |
| `H` / `X` / `S` | Sort by harness / context / tools |
| `+`, `-`, `=` | Speed up / slow down / reset refresh interval |
| `Space` | Mark / unmark the selected session |
| `D`, `K` | Delete / terminate all marked sessions (with confirmation) |
| `U` | Clear all marks |
| `h` or `F8` | Agent integration: what reports to cctop, and install it |
| `y` | Copy resume command or transcript path |
| `d` | Delete the selected session (not running) |
| `k` | Terminate the selected live session (with confirmation) |
| `s` | Type a line into the selected session's terminal (see below) |
| `R` | Resume the selected session in a tab of its own (see below) |
| `O` | Hand the selected session's context off to a different agent (see below) |
| `a` | Open that session's terminal in a tab and drive it |
| `t` | New tab: run an agent or a shell (see below) |
| `Esc` | Clear the active filter |
| `q` or `F10` | Quit |

Tabs and splits, from anywhere including inside a running agent:

| Key | Action |
|---|---|
| `t` or `Alt+n` | New tab: an agent, a shell, or one still running |
| `Alt+v` / `Alt+s` | Split the current tab right / down |
| `Alt+←` / `Alt+→` | Previous / next tab |
| `Alt+1`–`9` | Jump to a tab; `Alt+1` is the dashboard |
| `Alt+o` | Move focus to the next pane |
| `Alt+w` | Close the focused pane and stop its agent |
| `Alt+Shift+W` | The same thing, by a name that says so |
| `F12` | Back to the dashboard, leaving everything running |

Mouse works too: click session rows, column headers, and panel tabs; scroll
anywhere. In Tool Activity, click any row to expand the full untruncated
argument, and click the sidebar to filter by tool.

### Finding a session

`/` filters the table as you type, on everything a row is: its label or title,
the full working directory (not just the abbreviation the column has room for),
the git branch, the model, the harness, the provider and the session id. The
cell that matched is underlined, so it is clear *why* a row survived the filter.
`n` and `N` step through matches, `Esc` clears, and `↑`/`↓` inside the prompt
bring back a search you ran before — the last twenty are remembered across runs.

`Tab` widens the search to the transcripts themselves, which is how you find the
session where something was actually discussed rather than one whose name
happens to mention it. Transcript matches are added to the metadata matches
rather than replacing them, and the line each one was found on is shown under
the prompt.

This reads every transcript on disk, so it is opt-in, it waits for a pause in
typing and for a query of at least three characters, and it runs on cctop's
background thread pool — the table stays live throughout, and the footer says
`+transcripts…` while a scan is out. Results are remembered per query, so
refining a search re-reads only what it must. Two limits are worth knowing:
transcripts store their text as JSON, so a phrase containing a quote or a
newline is escaped on disk and will not match; and a single session is scanned
up to 64 MiB.

### Resuming a session

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

### Typing into a session

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

### Tabs and splits

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

#### Letting the agents tell cctop directly

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
| **Claude Code** | `~/.claude/settings.json`, or `<project>/.claude/` | nine hooks — `Stop`, `Notification`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `SessionStart`, `SessionEnd`, `PreCompact`, `SubagentStop` |
| **Gemini CLI** | `~/.gemini/settings.json`, or `<project>/.gemini/` | seven — `BeforeAgent`, `AfterAgent`, `BeforeTool`, `Notification`, `SessionStart`, `SessionEnd`, `PreCompress` |
| **Cursor** | `~/.cursor/hooks.json`, or `<project>/.cursor/` | seven — `stop`, `beforeSubmitPrompt`, `beforeShellExecution`, `sessionStart`, `sessionEnd`, `preCompact`, `subagentStop` |
| **Codex** | `~/.codex/config.toml` | `notify = ["cctop", "hook", "codex"]`, its one turn-complete report |
| **OpenCode** | `~/.config/opencode/plugins/cctop.ts`, or `<project>/.opencode/` | a plugin, since OpenCode extends by code rather than by command |

Each hook runs `cctop hook <event>`; the plugin and Codex's `notify` hand the
event over as an argument instead. Whichever way it arrives, it is reduced to
the same three facts — which session, what happened, which directory — and sent
to every running cctop over a unix socket. The agents disagree about all three
spellings (`session_id`, `thread-id`, `conversation_id`, `sessionID`; a `cwd` or
a `workspace_roots` array), so each is read under every name anyone uses.

If you installed before an event was added — `PostToolUse`, or a whole agent —
the panel shows that install as partial, and installing again fills it in.
Cursor also reads Claude Code's `settings.json` of its own accord, so with both
installed each moment arrives twice; that costs a process spawn and nothing
else, since the second event says exactly what the first did.

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

Panes cctop started are cctop's to end: closing one (`Alt+w`, or the agent
exiting on its own) takes the agent with it, tmux session included. Quitting
cctop is the opposite and leaves them running. A pane opened onto someone
else's session with `a` only stops watching.

Everything not in the table above goes to the focused agent, `Ctrl-C` included:
inside a pane, that interrupts the agent rather than quitting cctop. `F12` and
`Alt` are what cctop keeps — the function keys because they are the ones agents
never want, and `Alt` because `Ctrl` is the agent's.

### Getting pinged when a session needs you

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

### When two agents are in one repository

Two agents editing one checkout is not a merge conflict. Git would at least
announce that. It is one of them writing a file the other is still holding in
context, and the loser finds out when the work is already gone. cctop is the
only thing on the machine that can see both of them, so it is the only thing
that can say so while it still helps.

The `!` column is that warning:

| | |
|---|---|
| `⚠` | another running agent has written a file this session also wrote |
| `·` | another running agent is in the same repository, and has not touched your files |
| (blank) | nobody else is here |

Sort by it with `F6`, and the Info panel names the peer and lists the files. The
footer carries the `⚠` case only — agents share repositories all day and nothing
has gone wrong yet, whereas two of them writing one file means an edit has
already been lost or is about to be.

The unit of comparison is the **repository root**, not the working directory. A
linked worktree carries its own `.git`, so two agents in two worktrees of one
repository are editing two sets of files on disk and are not reported; two
agents started from different subdirectories of one checkout are. Comparing
directories gets both of those backwards, and the second is the arrangement
`git worktree` exists to provide.

Three limits worth knowing. Only running sessions are compared — a session that
has stopped may well have left uncommitted work behind, but nothing it does from
here can race anyone. Only the last 32 files each session wrote are watched, so
a path it finished with an hour and forty edits ago is not treated as contested.
And a Codex `apply_patch` covering several files summarises as
`first.rs (+3 more)` in the transcript, so only the first of them is recovered.

Agents can ask this themselves through `check_conflicts` — see
[Letting agents see each other](#letting-agents-see-each-other).

## Where data comes from

| Source | Path |
|--------|------|
| Claude Code (CLI) | `~/.claude/projects/<slug>/<uuid>.jsonl` |
| Claude for Mac | `~/Library/Application Support/Claude/{claude-code,local-agent-mode}-sessions/` |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| Cursor | `~/.cursor/projects/*/agent-transcripts/*/*.jsonl` |
| Gemini CLI | `~/.gemini/tmp/<project>/chats/session-*.json{,l}` |
| OpenCode | `~/.local/share/opencode/opencode*.db` (platform data directory) |
| Pi | `~/.pi/agent/sessions/**/*.jsonl` |
| Windsurf | `<Windsurf User dir>/workspaceStorage/*/state.vscdb` |

`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `CURSOR_HOME`, `GEMINI_DIR`,
`OPENCODE_DATA_DIR`, `PI_CODING_AGENT_DIR`, `PI_CODING_AGENT_SESSION_DIR`, and
`WINDSURF_USER_DIR` are honoured. Caches live in `~/.cache/cctop/`.

Gemini and Windsurf sessions are read from disk but not matched to a running
process: neither takes a session id on its command line, so there is nothing to
tie a PID to a transcript. Their rows always read as stopped, and the CPU and
memory columns stay blank rather than showing another session's figures.

The left status dot is green while an agent is working, amber after its latest
response is waiting for your input, and red when the newest transcript event is
an API error. A hollow grey dot is a stopped session, and a filled `◉` is the
session that rang in the last 30 seconds.

## A note on cost figures

Claude, Codex, and Gemini costs are **estimates**: tokens multiplied by published
per-token rates, taken from built-in tables and falling back to the
[LiteLLM](https://github.com/BerriAI/litellm) database (cached for 24 hours).
OpenCode and Pi already persist provider-calculated costs, which cctop reads
directly.

Gemini records a per-turn token breakdown but no cost. Its `cached` count is the
part of the prompt served from context cache rather than an addition to it, so
only the uncached remainder is priced at the full input rate; thinking tokens are
priced as output, which is how Google bills them. A Gemini session on a bundled
Code Assist tier will still show an estimate — the transcript carries nothing
that says which tier it ran on, and the figure is honest about what the tokens
would cost at retail.

Cursor native-agent transcripts expose projects, conversation activity, and
tool calls, but not model names, tokens, context usage, costs, or a dedicated
per-session process. Those fields display as unavailable; live status means the
transcript has changed within the last 90 seconds.

Windsurf goes further: its workspace state records the conversation and its tool
calls but no model, tokens, or cost at all — the credit accounting lives on
Codeium's servers. Those sessions report cost as unavailable rather than as free,
and their timestamps come from the workspace database's mtime, which is the only
clock Windsurf leaves behind.

Subscription plans — Claude Max, Pro, Team — are flat-rate or bundle tokens
differently, so these numbers will not match your invoice. Treat the `$` column
as a measure of resource consumption, not as billing. Use `--plan max` or
`--plan included` to display bundled usage as `incl` instead.

## Tool Activity columns

Each invocation shows the time, its arguments, and — where the transcript
supports it — what it did:

```
19:16 main    ~/cctop/src/ui/render.rs     +43 -24   122ms ↓498.5K ↑ 1.2K
19:34 ↳aa1b82 ~/cctop/src/quota.rs          +2 -0    88ms ↓ 41.2K ↑  310
19:41✗main    cargo test --all-targets               1.4s  ↓ 12.0K ↑  180
```

- **origin** — `main` for the session itself, or `↳<agent-id>` for a subagent.
  Subagent activity is interleaved into the same log, so without this there's no
  way to tell an agent's edits from the parent's.
- **`✗` and a red row** — the call reported an error. Claude records this per
  call, OpenCode and Gemini record a tool status, and Codex is read from the
  sandbox's own result line and exit code. Cursor and Windsurf transcripts don't
  record tool outcomes, so their calls are never marked.
- **`+N -M`** — lines added and removed, from the edit result's patch.
  Press `v` to expand the diff inline beneath the row.
- **duration** — wall time from the call being issued to its result arriving.
- **`↓` / `↑`** — tokens in and out for the assistant turn that issued the call.
  Claude only; Codex transcripts don't tie token counts to individual calls.

That last one deserves a caveat: **billing is per API request, not per tool
call.** When one turn issues several calls they all show that turn's figures,
marked with a leading `*`. Dividing the total between them would invent
precision the transcript doesn't contain. `↓` includes cache reads, which is why
it tracks total context size rather than the size of any one call.

Codex tools are decoded too: `apply_patch` shows the files it touched and its
line counts, `update_plan` shows progress and the step in flight, and
`write_stdin` distinguishes a real write from a poll for more output.

## Context breakdown

`CTX%` says the window is 68% full. The **Context** panel says what is in it —
Claude sessions only, since no other provider's transcript reports per-request
usage.

```
Window   181.4K of 200K

Unaccounted    ━━━━━━━━━━━━━━━━────────────────────────   62.1K  34%
Tool output    ━━━━━━━━━━━━────────────────────────────   43.5K  24%
Startup        ━━━━━━━━━───────────────────────────────   34.5K  19%
Tool input     ━━━━━━━━────────────────────────────────   29.0K  16%
Attachments    ━━━━────────────────────────────────────    9.1K   5%
Assistant text ━━──────────────────────────────────────    3.6K   2%
```

Two of those numbers are measured and the rest are estimated, and the panel
never blurs the line:

- **Window** and **Startup** come from the usage figures the API itself
  reported. Startup is the first request of the live segment — everything the
  harness sends before the conversation begins: the system prompt, the tool
  schemas, CLAUDE.md, the skills index, and, after a compaction, the summary. It
  cannot be split further, because the transcript never records what was sent,
  only that it was.
- **Tool output**, **Tool input**, **Attachments**, **Your messages** and
  **Assistant text** are estimated from how many characters the transcript
  holds, at 2.75 characters per token. That constant is fitted rather than
  assumed: across 167 local sessions, the characters a transcript accumulates
  divided by the context growth the API reports over the same span lands there,
  well under the usual prose rule of thumb because this content is mostly code,
  JSON and file paths.
- **Unaccounted** is the remainder, and it is deliberately a bar of its own
  rather than being spread across the categories that happen to be measurable.
  It runs around a third of the window. Most of it is thinking — Claude Code
  writes those blocks with the text stripped and only the signature left, so
  there is nothing to measure — plus the `<system-reminder>` text the harness
  splices into each turn without recording it, plus estimation error.

A compaction resets the whole thing: everything before the summary has left the
window, so counting across one would describe a context that no longer exists.
Subagent turns are excluded too — they run against their own windows, and only
the report a subagent hands back is in the parent's, where it lands in **Tool
output** like any other result.

When the estimate overshoots the window there is no gap to draw, and the panel
says so instead of clamping: it means the harness has dropped context that the
transcript still holds.

Under the bar, **How it filled** charts the window across every request the
session made. The bar answers "what is in there"; the chart answers "how did it
get that full", which is the part that changes what you do next. A window that
climbed evenly is a conversation that grew and will keep growing. One that
stepped is a handful of large tool results, and the same call will do it again.
A sawtooth is a session living on compactions, paying to rebuild its context
over and over. The chart spans the whole session rather than the live segment,
because a compaction is the most interesting thing that can happen to a context
window and it is the only view that can show one.

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

## Design notes

A few things that are less obvious from the code:

- **Token dedup.** Streaming writes the same `requestId` repeatedly with growing
  counts. Only the last entry per request is counted; summing them all inflates
  totals several-fold.
- **Cache keys carry a pricing generation.** Cached entries hold *computed*
  costs, so a refreshed rate table must invalidate them just as an appended
  transcript does. Without this, sessions priced before the table loaded report
  `$0.00` forever — their transcripts never change again.
- **Threads are excluded from process matching.** Threads share their process's
  command line, so every one of them matches the same session and competes to be
  picked as the root — nondeterministically. The winner reports its own CPU and
  no children.
- **Tail reads.** Context usage and last-tool come from seeking backwards from
  EOF, so a live 50 MB transcript costs one 64 KB read per refresh, not a reparse.
- **Ghost subagents.** Claude Code purges old subagent transcripts but keeps the
  `tool_use`/`tool_result` pair in the parent. Those rows are reconstructed and
  marked `◌`, with `—` rather than `0` for figures that can no longer be measured.
