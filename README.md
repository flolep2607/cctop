# cctop

**An htop for your AI coding agents.** One screen showing every Claude Code,
Codex, Cursor, Gemini CLI, OpenCode, Pi and Windsurf session on your machine —
what each is doing, what it has spent, and which one is waiting on you.

![cctop showing seven agent sessions with their cost, context usage, tool counts and error rates](docs/assets/dashboard.png)

It reads what the agents leave on disk, so it sees sessions it did not start,
including ones that ended weeks ago. Nothing to configure, nothing to run
alongside it.

A Rust rewrite of an earlier Node implementation.

## Install

```bash
cargo install cctop
```

Or grab a binary for your platform from the
[latest release](https://github.com/flolep2607/cctop/releases/latest):

```bash
# Linux x86_64 (static — works on any distro)
curl -fsSL https://github.com/flolep2607/cctop/releases/latest/download/cctop-x86_64-unknown-linux-musl.tar.gz | tar xz
sudo install -m755 cctop /usr/local/bin/cctop
```

macOS, Windows, checksums and `cctop --update` are in
[Installing cctop](docs/install.md).

## Start here

```bash
cctop
```

That is the whole first run. It finds your sessions, prices them, and draws the
table above.

Four keys are worth knowing before anything else:

| Key | |
|---|---|
| `↑` `↓` | move between sessions |
| `←` `→` | move between the panels underneath |
| `/` | filter, on anything a row is |
| `q` | quit |

Then two commands worth running once:

```bash
cctop --install-hooks   # let the agents report their own state, live
cctop doctor            # check the installation and say what is wrong with it
```

## What it tells you

**What everything costs.** Tokens times published rates, per session, per hour,
per day, per model. The numbers are estimates and the
[cost page](docs/costs.md) is honest about which providers report real figures
and which are inferred.

**What is in the context window.** `CTX%` says it is 68% full; the Context panel
says what is *in* it — startup, tool output, attachments, and an
Unaccounted bar that never pretends to be smaller than it is. Under it, a chart
of how the window filled across the whole session, compactions included.
See [The bottom panels](docs/panels.md).

**Which session needs you.** The status dot goes amber when an agent is waiting
on input and red on an API error. `w` turns on the terminal bell and a desktop
notification for the moment a session crosses into waiting.

**Which sessions are stuck.** `ERR%` is the share of a session's tool calls that
failed — a quarter of them is an agent retrying something that will not work and
paying for each attempt. Compaction cadence catches the other kind of waste, a
session rebuilding a window it keeps refilling.

**When two agents are about to collide.** Two agents in one checkout is not a
merge conflict — git would announce that. It is one of them writing a file the
other still holds, and the loser finds out when the work is gone. The `!` column
says so first. See [Reading the table](docs/the-table.md#-when-two-agents-are-in-one-repository).

## What it can do

Beyond watching, cctop can answer an agent (`s`), reopen any session in a tab of
its own (`R`), hold several agents side by side, hand a session's context over to
a *different* harness (`O`), and read the sessions on another machine over ssh.

- [Reading the table](docs/the-table.md) — every column, the status dot, filtering, and the full key list
- [Driving agents](docs/driving-agents.md) — typing into sessions, resuming, tabs and splits, notifications, handoff
- [The bottom panels](docs/panels.md) — Tool Activity and the context breakdown
- [What the cost figures mean](docs/costs.md) — how each provider is priced, and where the data comes from
- [Integrations](docs/integrations.md) — agent hooks, the MCP server, and `--host`
- [Troubleshooting](docs/troubleshooting.md) — `cctop doctor` and the usual causes

## Which agents it reads

| Agent | Cost | Tokens | Context | Tools | Live process |
|---|---|---|---|---|---|
| Claude Code | estimated | ✓ | ✓ full breakdown | ✓ | ✓ |
| Codex | estimated | ✓ | ✓ | ✓ | ✓ |
| OpenCode | reported | ✓ | ✓ | ✓ | ✓ |
| Pi | reported | ✓ | ─ | ✓ | ✓ |
| Gemini CLI | estimated | ✓ | ─ | ✓ | ─ |
| Cursor | ─ | ─ | ─ | ✓ | inferred |
| Windsurf | ─ | ─ | ─ | ✓ | ─ |

Claude for Mac is read too. A `─` is a gap in what that harness records, not in
cctop — the details are on the [cost page](docs/costs.md).

## A note on cost figures

Most costs are **estimates**: tokens multiplied by published per-token rates.
Subscription plans — Claude Max, Pro, Team — are flat-rate or bundle tokens
differently, so these numbers will not match your invoice. Treat the `$` column
as a measure of resource consumption, not as billing. `--plan max` displays
bundled usage as `incl` instead.

## Usage

```bash
cctop                 # interactive UI
cctop --list          # print a table and exit
cctop --json          # dump full session data as JSON
cctop --plan max      # treat Claude usage as bundled
cctop --host devbox   # also show another machine's sessions, read over ssh
cctop doctor          # check this installation and say what is wrong with it
cctop claude          # start an agent on a pty cctop can watch and type into
```

`cctop --help` has the rest.

## Contributing

Bug reports and patches welcome — see [CONTRIBUTING.md](CONTRIBUTING.md), which
also has the architecture notes and the things about this codebase that are not
obvious from reading it.

## License

[MIT](LICENSE).
