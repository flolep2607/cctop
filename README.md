# cctop

An htop-like monitor for AI coding agent sessions. Tracks Claude Code, Codex,
Cursor, OpenCode, and Pi sessions on your machine: cost estimation, token usage,
tool invocations, subagents, and OS-level process metrics — refreshed live.

For active sessions, the **HARNESS** column distinguishes the host application
(for example, Cursor) from the model. Unknown launchers remain `─` rather than
being guessed.

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
```

### Keys

| Key | Action |
|-----|--------|
| `↑`/`k`, `↓`/`j` | Move between sessions |
| `←`, `→` | Move between bottom panels |
| `1`–`7` | Jump to a panel directly |
| `Shift+↑`/`↓` | Scroll inside the active panel |
| `/` or `F3` | Filter sessions by text |
| `F6`, `>`, `<` | Sort-by panel |
| `F7` | Filter by age (1d / 1w / 1mo) |
| `` ` `` | Show only running sessions |
| `[`, `]` | Move through the Tool Activity tool filter |
| `v` | Toggle inline diffs for edits |
| `L` | Toggle the Tool Activity live filter |
| `P` / `M` / `T` | Sort by status / memory / cost |
| `y` | Copy resume command or transcript path |
| `d` | Delete the selected session (not running) |
| `Esc` | Clear the active filter |
| `q` or `F10` | Quit |

Mouse works too: click session rows, column headers, and panel tabs; scroll
anywhere. In Tool Activity, click any row to expand the full untruncated
argument, and click the sidebar to filter by tool.

## Where data comes from

| Source | Path |
|--------|------|
| Claude Code (CLI) | `~/.claude/projects/<slug>/<uuid>.jsonl` |
| Claude for Mac | `~/Library/Application Support/Claude/{claude-code,local-agent-mode}-sessions/` |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| Cursor | `~/.cursor/projects/*/agent-transcripts/*/*.jsonl` |
| OpenCode | `~/.local/share/opencode/opencode*.db` (platform data directory) |
| Pi | `~/.pi/agent/sessions/**/*.jsonl` |

`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `CURSOR_HOME`, `OPENCODE_DATA_DIR`,
`PI_CODING_AGENT_DIR`, and `PI_CODING_AGENT_SESSION_DIR` are honoured. Caches
live in `~/.cache/cctop/`.

The left status dot is green while an agent is working, amber after its latest
response is waiting for your input, and red when the newest transcript event is
an API error. A hollow grey dot is a stopped session.

## A note on cost figures

Claude and Codex costs are **estimates**: tokens multiplied by published
per-token rates, taken from built-in tables and falling back to the
[LiteLLM](https://github.com/BerriAI/litellm) database (cached for 24 hours).
OpenCode and Pi already persist provider-calculated costs, which cctop reads
directly.

Cursor native-agent transcripts expose projects, conversation activity, and
tool calls, but not model names, tokens, context usage, costs, or a dedicated
per-session process. Those fields display as unavailable; live status means the
transcript has changed within the last 90 seconds.

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
```

- **origin** — `main` for the session itself, or `↳<agent-id>` for a subagent.
  Subagent activity is interleaved into the same log, so without this there's no
  way to tell an agent's edits from the parent's.
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
