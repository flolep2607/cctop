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

cctop checks for a new release once a day in the background and, when one
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
| `/` or `F3` | Filter sessions by text |
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
| `y` | Copy resume command or transcript path |
| `d` | Delete the selected session (not running) |
| `k` | Terminate the selected live session (with confirmation) |
| `s` | Type a line into the selected session's terminal (see below) |
| `a` | Open that session's terminal in a tab and drive it |
| `t` | New tab: run an agent or a shell (see below) |
| `Esc` | Clear the active filter |
| `q` or `F10` | Quit |

Tabs and splits, from anywhere including inside a running agent:

| Key | Action |
|---|---|
| `t` or `Alt+n` | New tab: run an agent or a shell |
| `Alt+v` / `Alt+s` | Split the current tab right / down |
| `Alt+←` / `Alt+→` | Previous / next tab |
| `Alt+1`–`9` | Jump to a tab; `Alt+1` is the dashboard |
| `Alt+o` | Move focus to the next pane |
| `Alt+w` | Close the focused pane |
| `F12` | Back to the dashboard, leaving everything running |

Mouse works too: click session rows, column headers, and panel tabs; scroll
anywhere. In Tool Activity, click any row to expand the full untruncated
argument, and click the sidebar to filter by tool.

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
`git diff` are one keystroke apart. A tab opened from a session's row starts in
that session's project directory.

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

A tab you are not looking at blinks when it wants you: **green** when its agent
has stopped drawing — its turn is over and the prompt is yours — and **amber**
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

#### Letting Claude Code tell cctop directly

Both of those are cctop guessing. Claude Code can just say it:

```bash
cctop --install-hooks     # merge cctop's hooks into ~/.claude/settings.json
cctop --remove-hooks      # take them back out
```

That registers four hooks — `Stop`, `Notification`, `UserPromptSubmit`,
`PreToolUse` — each running `cctop hook <event>`, which forwards the event to a
running cctop over a unix socket. A reported turn beats a still screen: the
green appears the instant the turn ends rather than two seconds later, and the
amber no longer waits for a transcript to be written. Sessions already running
keep their old hooks until they restart.

The installer merges into your settings rather than writing them, matches its
own entries by command text so it is idempotent and removable, writes through a
temporary file so an interrupted write cannot leave you with no settings, and
refuses outright if the file is not valid JSON rather than replacing it.

**`cctop hook` cannot break your session.** An agent reads a hook's exit code as
a decision — exit 2 blocks the tool call — so this one exits 0 unconditionally,
writes nothing to stdout, and is bounded by a 250ms deadline covering the whole
exchange, on a thread the process abandons if it overruns. No cctop running, a
stale socket, malformed input, a wedged cctop, an outright panic: every one of
them is a silent, prompt success. Dropping an event is always cheaper than
stalling an agent.

Panes cctop started are cctop's to end: closing one (`Alt+w`, or the agent
exiting on its own) takes the agent with it, and quitting cctop takes them all.
A pane opened onto someone else's session with `a` only stops watching.

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
