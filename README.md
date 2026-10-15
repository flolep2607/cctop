# cctop

[![crates.io](https://img.shields.io/crates/v/cctop?style=flat-square)](https://crates.io/crates/cctop)
[![CI](https://github.com/flolep2607/cctop/actions/workflows/ci.yml/badge.svg)](https://github.com/flolep2607/cctop/actions/workflows/ci.yml)
[![Linux](https://img.shields.io/badge/platform-Linux-blue?style=flat-square)](#install)
[![license](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

**htop for your AI coding agents.** One terminal screen listing every Claude
Code, Codex, Cursor, Devin, Gemini CLI, OpenCode, Pi and Windsurf session on
your machine: what each is doing, what it has cost, and which one is waiting on
you. You can open any of them in a tab and type into it.

![cctop: walking the session table, opening the context breakdown, then filtering](docs/assets/demo.gif)

<sub>A real recording. [Play it in a terminal](docs/assets/demo.cast) with
`asciinema play docs/assets/demo.cast`.</sub>

It reads what the agents already write to disk, so it sees sessions it did not
start, including ones that ended weeks ago. Nothing to configure first.

## Install

Linux only, WSL included. There is no macOS or Windows build.

```bash
curl -fsSL https://raw.githubusercontent.com/flolep2607/cctop/main/install.sh | sh
```

The script picks the x86_64 or aarch64 release, checks its checksum and puts
one static binary in `/usr/local/bin` (or `~/.local/bin`). It then offers to
install `sshfs`, which only `cctop sandbox` needs; Enter says no.

<details>
<summary>Other ways to install</summary>

From the release archive, by hand (swap in `aarch64` on ARM):

```bash
d=$(mktemp -d)
curl -fsSL https://github.com/flolep2607/cctop/releases/latest/download/cctop-x86_64-unknown-linux-musl.tar.gz | tar xz -C "$d"
sudo install -m755 "$d/cctop" /usr/local/bin/cctop && rm -rf "$d"
```

With cargo:

```bash
cargo binstall cctop           # the release binary
cargo install cctop --locked   # compile it; needs Rust 1.89 or newer
```

### From source

```bash
git clone https://github.com/flolep2607/cctop && cd cctop
cargo build --release          # binary in target/release/cctop
```

Checksums and updating (`cctop --update`) are in
[Installing cctop](docs/install.md).

</details>

## First steps

### 1. Run it

```bash
cctop
```

It finds your sessions, prices them and draws the table. The keys to start
with:

| Key | |
|---|---|
| `↑` `↓` | move between sessions |
| `Enter` | everything you can do to the selected session, in one menu |
| `/` | filter on anything in a row |
| `R` | resume the selected session in a tab of its own |
| `s` | type a line into a running agent |
| `t` | new tab: an agent or a shell, here or on another machine |
| `Alt+b` | jump to the tab that is waiting on you |
| `F12` or `Alt+1` | back to the dashboard; the tab keeps running |
| `?` | every key |
| `q` | quit |

Agents in cctop's tabs run in its built-in multiplexer, so they keep running
after you quit and are back in their tabs next time.

### 2. Install the hooks

```bash
cctop --install-hooks
```

Claude Code, Codex, Gemini CLI, Cursor and OpenCode then report their state to
cctop as it happens, instead of cctop inferring it from disk. `cctop doctor`
says what is wrong when something looks off.

### 3. Use it from your phone

Press `B` in the dashboard (or run `cctop serve --tunnel`) to serve the same
table, and a page per session, to a browser. `t` in that panel adds a public
link through Cloudflare, and `c` shows it as a QR code to scan. Whoever holds
the link can drive your agents; a second, read-only link is printed beside it.

For a link that stays the same across runs, connect your own free Cloudflare
account once (it needs a domain whose DNS is on Cloudflare):

```bash
cctop tunnel setup --browser
```

Details, Cloudflare Access logins and invites: [In a browser](docs/serve.md).

### 4. Several machines, one table

Run `cctop tunnel setup --browser` on each machine, against the same Cloudflare
account, and keep each one serving on its tunnel (`B`, or
`cctop serve --tunnel`). Each then shows the others' sessions in its table,
with the machine's name on the row. No ssh route is needed. `R` opens a remote
session in a tab here, and `t` starts a new agent on any of them.

Over ssh instead: `cctop --host devbox` adds another machine's sessions,
read-only.

## Features

- **Cost.** Tokens times published rates, per session, hour, day and model.
- **Context.** `CTX%` shows how full each context window is; the Context panel
  shows what fills it.
- **Waiting on you.** The status dot goes amber when an agent needs a reply,
  and turns to `✓` when a turn ended while you were looking elsewhere.
- **Stuck work.** `ERR%` is the share of a session's tool calls that failed.
- **Collisions.** The `!` column warns when two agents edit the same files in
  one checkout.
- **Terminals.** Tabs are real terminals you can type into, split and drag.
- **Handoff.** `O` hands a session's context to a different agent or account.
- **What the money bought.** [`cctop optimize`](docs/optimize-and-compare.md)
  finds spend that bought nothing, [`cctop compare`](docs/optimize-and-compare.md)
  puts your models side by side on your own work, and
  [`cctop yield`](docs/yield.md) checks which sessions' work reached the
  default branch.
- **Recall.** [`cctop recall "<query>"`](docs/integrations.md) finds what past
  sessions said about something; `cctop --install-mcp` gives the agents the
  same.

> [!NOTE]
> Costs are estimates: tokens times published per-token rates. Subscription
> plans (Claude Max, Pro, Team) bill differently, so the `$` column will not
> match your invoice. `--plan max` shows bundled usage as `incl`.
> [What the cost figures mean](docs/costs.md).

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

A `─` marks something the harness does not record. Each
[provider's page](docs/providers/) has the detail.

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
cctop sandbox devbox:/srv/api   # Claude here, its commands and files on devbox
cctop doctor          # check this installation and say what is wrong with it
cctop serve           # the same table in a browser
cctop tunnel setup    # your own Cloudflare tunnel, for a link that stays the same
cctop claude          # start an agent on a pty cctop can watch and type into
```

`cctop --help` has the rest.

</details>

## Documentation

- [Reading the table](docs/the-table.md): every column, the status dot,
  filtering, and the full key list
- [Driving agents](docs/driving-agents.md): typing into sessions, resuming,
  tabs and splits, notifications
- [In a browser](docs/serve.md): `cctop serve`, tunnels, phones, several
  machines
- [The bottom panels](docs/panels.md): Tool Activity and the context breakdown
- [Installing cctop](docs/install.md) · [troubleshooting](docs/troubleshooting.md)

More: [what it cost you for](docs/optimize-and-compare.md)
· [did the spend ship](docs/yield.md) · [what the subscription bought](docs/subscription-burn.md)
· [what the cost figures mean](docs/costs.md) · [provider by provider](docs/providers/)
· [integrations](docs/integrations.md)

## Telemetry

cctop sends a small usage report at most once a day to
`https://cctop-releases.ecorsiste.workers.dev/telemetry`. It is on by default,
and nothing is sent until a one-time notice has been shown on an interactive
run. `cctop hook` and `cctop yolo-hook` never send anything. A report holds
only:

- `id`: 128 random bits, made at the first report; `cctop telemetry reset-id`
  replaces it.
- `version`, `os`, `arch`: cctop's version, and e.g. `linux`, `x86_64`.
- `features`: how many times each of `tui`, `serve`, `workspace`, `tunnel`,
  `yolo` and `update` ran since the last report.
- `agents`: the most agents of each kind seen running at once.
- `crash`, only if cctop panicked: the file and line in cctop's source, and
  the panic message with paths, URLs, emails, user and host names, IPs and long
  tokens removed, cut to 200 characters.

It never sends prompts, session names, working directories, account names,
hostnames or IP addresses. `cctop telemetry status` shows the next report.
To turn it off, any one of: `cctop telemetry off` (also deletes the ID and the
counts), `telemetry = false` under `[settings]` in `config.toml` or the toggle
in settings (`,`), or `CCTOP_TELEMETRY=0` / `DO_NOT_TRACK=1`.
