# cctop

**An htop for your AI coding agents.** One screen showing every Claude Code,
Codex, Cursor, Devin, Gemini CLI, OpenCode, Pi and Windsurf session on your
machine: what each is doing, what it has spent, and which one needs you.

![cctop: walking the session table, opening the context breakdown, then filtering](docs/assets/demo.gif)

<sub>A real recording. [Play it in a terminal](docs/assets/demo.cast) with
`asciinema play docs/assets/demo.cast`.</sub>

It reads what the agents leave on disk, so it sees sessions it did not start,
including ones that ended weeks ago. Nothing to configure, nothing to run
alongside it.

## What you get

- **What everything costs.** Tokens times published rates, per session, per
  hour, per day, per model.
- **What is in the context window.** `CTX%` says it is 68% full. The Context
  panel says what is *in* it, including an Unaccounted bar that never pretends
  to be smaller than it is.
- **Which session needs you.** The status dot goes amber when an agent is
  waiting, and turns to a `✓` when a turn ended while you looked elsewhere.
- **Which sessions are stuck.** `ERR%` is the share of a session's tool calls
  that failed. A quarter of them means an agent is retrying something that will
  not work, paying for each attempt.
- **When two agents are about to collide.** Two agents in one checkout do not
  make a merge conflict. Git would announce that. They make one agent overwrite
  a file the other still holds.

## Install

**cctop runs on Linux, including WSL.** It reads Linux process tables and drives
agents over ptys and unix sockets. There is no macOS or Windows build.

```bash
curl -fsSL https://raw.githubusercontent.com/flolep2607/cctop/main/install.sh | sh
```

<details>
<summary>Other ways to install</summary>

By hand, from a statically linked release archive. Swap in the aarch64 name on
that architecture.

```bash
d=$(mktemp -d)
curl -fsSL https://github.com/flolep2607/cctop/releases/latest/download/cctop-x86_64-unknown-linux-musl.tar.gz | tar xz -C "$d"
sudo install -m755 "$d/cctop" /usr/local/bin/cctop && rm -rf "$d"
```

```bash
cargo binstall cctop   # fetches the release binary
cargo install cctop    # or compiles it; needs Rust 1.88 or newer
```

From source: `git clone`, `cd cctop`, `cargo build --release`. The binary lands
in `target/release/cctop`.

Checksums and `cctop --update` are in [Installing cctop](docs/install.md).

</details>

## Start here

```bash
cctop
```

That is the whole first run. It finds your sessions, prices them, and draws the
table you saw above. Six keys are worth knowing before anything else:

| Key | |
|---|---|
| `↑` `↓` | move between sessions |
| `←` `→` | move between the panels underneath |
| `/` | filter, on anything a row is |
| `R` | reopen the selected session in a tab of its own |
| `F12` or `Alt+1` | back to the dashboard, leaving the tab running |
| `q` | quit |

Two commands worth running once:

```bash
cctop --install-hooks   # let the agents report their own state, live
cctop doctor            # check the installation and say what is wrong with it
```

## Going further

A tab is a real terminal: type into it, split it with `Alt+v` and `Alt+s`, drag
it along the bar. Right-click a tab, or press `Alt+r`, to give it a name and a
colour, and every cctop on the machine shows it. Past more tabs than digits,
`Alt+t` picks one from a list you narrow by typing. `Alt+b` jumps to whichever
agent is waiting on you, and `Alt+z` zooms a pane over the whole tab and back.

Beyond watching, cctop can answer an agent (`s`), hand a session's context to a
different harness (`O`), read the sessions on another machine over ssh
(`--host`), and stream the table to a browser. On a phone that last one earns
its place, since the sessions waiting on you can find *you*.

It can also tell you what the money bought.

- [`cctop optimize`](docs/optimize-and-compare.md) finds the reads into
  `node_modules`, the files fetched again after a compaction, and the tool calls
  that failed and were billed anyway. Each finding carries its cost and says
  whether that figure was measured or estimated.
- [`cctop compare`](docs/optimize-and-compare.md) puts your models side by side
  on your own work: how often each got a file right first time, what a changed
  file cost, how long it took. `--rate 60` prices that time too.
- [`cctop yield`](docs/yield.md) asks the repository what became of the spend:
  which sessions' work reached the default branch, which sits on a side branch,
  which was never committed.
- [`cctop recall "why is the cache sharded"`](docs/integrations.md) returns the
  passages of past sessions that discussed it. `cctop --install-mcp` gives the
  same to the agents themselves.

## Documentation

- [Reading the table](docs/the-table.md) - every column, the status dot,
  filtering, and the full key list
- [Driving agents](docs/driving-agents.md) - typing into sessions, resuming,
  tabs and splits, notifications
- [In a browser](docs/serve.md) - `cctop serve`, the session report, and
  reaching it from a phone
- [The bottom panels](docs/panels.md) - Tool Activity and the context breakdown

Then, if you want the detail: [what it cost you for](docs/optimize-and-compare.md)
· [did the spend ship](docs/yield.md) · [what the subscription bought](docs/subscription-burn.md)
· [what the cost figures mean](docs/costs.md) · [provider by provider](docs/providers/)
· [integrations](docs/integrations.md) · [troubleshooting](docs/troubleshooting.md)

## Reference

<details>
<summary>Which agents it reads, and what each one records</summary>

| Agent | Cost | Tokens | Context | Tools | Live process |
|---|---|---|---|---|---|
| Claude Code | estimated | ✓ | ✓ full breakdown | ✓ | ✓ |
| Codex | estimated | ✓ | ✓ | ✓ | ✓ |
| OpenCode | reported | ✓ | ✓ | ✓ | ✓ |
| Pi | reported | ✓ | ─ | ✓ | ✓ |
| Gemini CLI | estimated | ✓ | ─ | ✓ | ─ |
| Devin | ─ | ✓ | ✓ | ✓ | ✓ |
| Cursor | ─ | ─ | ─ | ✓ | inferred |
| Windsurf | ─ | ─ | ─ | ✓ | ─ |

A `─` marks something the harness does not record, which is a different thing
from something cctop cannot show. Each [provider's page](docs/providers/) has the
detail. Devin records tokens, context, tools and a live process, and no money at
all.

</details>

<details>
<summary>A note on cost figures</summary>

Most costs are estimates: tokens multiplied by published per-token rates.
Subscription plans (Claude Max, Pro, Team) charge a flat rate or bundle a fixed
allowance, so these numbers will not match your invoice. Treat the `$` column
as a measure of resource consumption rather than billing. `--plan max` shows
bundled usage as `incl`. [What the cost figures mean](docs/costs.md) is honest
about which providers report real figures and which get inferred.

</details>

<details>
<summary>Commands</summary>

```bash
cctop                 # interactive UI
cctop --list          # print a table and exit
cctop --json          # dump full session data as JSON
cctop --statusline    # one line for a status bar: "3 working · 1 waiting · $4.12/h"
cctop --plan max      # treat Claude usage as bundled
cctop --host devbox   # also show another machine's sessions, read over ssh
cctop doctor          # check this installation and say what is wrong with it
cctop serve           # the same table in a browser, on a port or a phone
cctop claude          # start an agent on a pty cctop can watch and type into
```

`cctop --help` has the rest.

</details>

## Contributing

Bug reports and patches welcome. [CONTRIBUTING.md](CONTRIBUTING.md) has the
architecture notes and the things about this codebase that reading it will not
tell you. If you send a pull request, name it after what changes for someone
using cctop, because the title becomes the release note that `cctop --update`
prints. [CLAUDE.md](CLAUDE.md) says what reads well there, and it applies to
coding agents as much as to people.

## License

[MIT](LICENSE).