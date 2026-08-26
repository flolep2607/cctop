---
name: run-cctop
description: Build, run, drive, and screenshot cctop — the terminal dashboard for AI coding agent sessions. Use when asked to start cctop, launch its TUI, take a screenshot of it, exercise a UI change, run its tests, or check the release gate.
---

cctop is a Rust TUI that watches *other* agents' sessions, so running it needs
two things a clean machine lacks: sessions to show, and a terminal to draw in.
`.claude/skills/run-cctop/driver.sh` supplies both — it writes a throwaway
`$HOME` full of fake transcripts and drives the TUI inside a private tmux
server. That tmux is the driver's terminal, not cctop's backend — cctop hands
agents to rmux. Start there; `cargo run` on its own draws an empty table.

Paths are relative to the repo root.

## Prerequisites

Nothing needed installing in this container — Rust, tmux and python3 were
already present. Verify:

```bash
cargo --version   # 1.95.0
tmux -V           # 3.2a
python3 --version # 3.10.18 — used by the driver to pretty-print `cctop -j`
```

If tmux is absent: `sudo apt-get update && sudo apt-get install -y tmux`.

## Build

```bash
cargo build              # debug, what the driver runs
```

## Run (agent path)

One command does everything and asserts as it goes — build, fixture, headless
surfaces, launch, four screens, clean shutdown:

```bash
.claude/skills/run-cctop/driver.sh smoke
```

Screens land in `/tmp/cctop-drive/shots/*.txt` (plain text — a `capture-pane`
dump *is* the screenshot for a TUI). **Read them.** `dashboard.txt` must show
`Sessions (N)` with rows, not `Scanning for sessions…`.

Step by step, when you are exercising a specific change:

```bash
.claude/skills/run-cctop/driver.sh fixture          # throwaway $HOME with fake sessions
.claude/skills/run-cctop/driver.sh up               # launch, wait until a row is drawn
.claude/skills/run-cctop/driver.sh keys M-n         # Alt+n — the new-tab launcher
.claude/skills/run-cctop/driver.sh wait "New tab"   # poll, do not sleep
.claude/skills/run-cctop/driver.sh shot launcher    # writes + prints the screen
.claude/skills/run-cctop/driver.sh down             # F10, then kill the server
```

| command | what it does |
|---|---|
| `build` | `cargo build` |
| `fixture` | rewrites `/tmp/cctop-drive/home`: a Claude transcript, two Codex accounts (`.codex`, `.codex-work`), one Codex transcript, seeded pricing cache |
| `up [--spawn]` | launches `target/debug/cctop` on tmux socket `cctopdrv`, waits for a real row, presses F12 for safety |
| `keys <keys…>` | `tmux send-keys` — e.g. `keys M-n`, `keys Down`, `keys Escape`, `keys F10` |
| `wait <text> [secs]` | polls `capture-pane` for text; dumps the screen and fails if it never comes |
| `shot [name]` | `capture-pane` → `/tmp/cctop-drive/shots/<name>.txt`, also printed |
| `attach` | attaches your terminal to the running session (Ctrl-b d to detach) |
| `down` | F10, then kills the private tmux server |
| `headless` | `cctop doctor` and `cctop -j` against the fixture — no tmux needed |
| `smoke` | all of the above, with assertions |

### Keys worth knowing

| key | action |
|---|---|
| `Alt+n` | new-tab launcher (`Down` to pick a harness, `p` cycles its account, `Esc` closes) |
| `Enter` | row menu for the selected session (Resume / Attach / Type into / …) |
| `F12` | back to the dashboard from inside a pane — **the escape hatch** |
| `F10` | quit |
| `←`/`→` | switch the bottom panel (Activity, Subagents, Cost, Config) |

### Headless surfaces — no TUI at all

For a change to discovery, pricing or JSON output, skip tmux entirely:

```bash
env HOME=/tmp/cctop-drive/home CI=1 ./target/debug/cctop doctor   # where it reads from, what it found
env HOME=/tmp/cctop-drive/home CI=1 ./target/debug/cctop -j       # every row as JSON
env HOME=/tmp/cctop-drive/home CI=1 ./target/debug/cctop -l       # one line per row
```

## Run (human path)

```bash
cargo run          # draws your real sessions; F10 quits
```

Useless for an agent: it takes over the terminal and shows the operator's own
sessions rather than anything a test controls.

## Test

```bash
cargo test         # 475 passed
```

The release gate, which is what CI runs — a warning is a build failure there:

```bash
export RUSTFLAGS="-D warnings"
cargo fmt --all --check
cargo clippy --all-targets
cargo test
cargo publish --dry-run --allow-dirty
```

## Gotchas

- **`tmux new-session` does not pass your environment to the pane.** The pane
  inherits the *server's* env, and a server is usually already running, so
  `HOME=/tmp/x tmux new-session … cctop` silently gives cctop the real `$HOME` —
  it reads the operator's sessions and you never notice. Use `-e HOME=…`, as
  the driver does.
- **cctop adopts every cctop-owned `rmux` session on the machine as a tab.**
  Since 0.8 cctop drives rmux and not tmux, so the driver's own tmux server is
  invisible to it — but a real `cctop-*` rmux session belonging to the operator
  is not, and it lands in tab 2 displayed and *typeable*: inside a pane only the
  function keys stay cctop's, so a stray `Down` goes to that agent. The driver
  still uses a private tmux socket (`tmux -L cctopdrv`) and sends F12 on
  startup; if the machine has live `cctop-*` rmux sessions, expect them.
- **The launcher cannot start an agent from inside a multiplexer.** cctop runs
  `rmux new-session -A`, which refuses to nest under a `$RMUX`/`$TMUX` it can
  see; the status line says `Started codex …` and no session appears. cctop's
  own pty shim strips both pairs plus `TMUX_PROGRAM`, so an agent it launches is
  clean — what is not clean is a cctop the driver started *inside* tmux, which
  is why `up --spawn` runs it through a wrapper that unsets `TMUX`. Any session
  that wrapper starts outlives the driver, on the rmux daemon.
- **`CI=1` skips the first-run prompt** that offers to write shell aliases into
  `~/.zshrc` and `~/.bashrc` (`src/main.rs:254`). Without it the app waits on
  stdin for `y`/`n` behind the alias question and never draws. Never run
  `cctop --install-alias` while testing — it edits real rc files.
- **The MODEL column is shortened**, so `claude-opus-5` renders as `opus-5`.
  Waiting on the long name never matches — the driver polls for
  `gpt-5.6-terra`, which survives shortening.
- **Rows can come from the process table, not a transcript.** A live agent
  anywhere on the machine shows up even under a fixture `$HOME`, with
  `session_id: _pid_<pid>` and empty model/cost. Assert on your fixture's own
  values, never on the row count.
- **A fresh fixture home has no pricing cache**, so every model costs `$0.00` —
  indistinguishable from a free plan. `fixture` copies the operator's cached
  LiteLLM table when there is one.
- **`cargo run` breaks under an overridden `$HOME`**: rustup looks for its
  toolchains there and reports `could not choose a version of cargo to run`.
  Run `./target/debug/cctop` directly, as the driver does.
- **`cctop doctor` exits 1 when any check is ✗.** That is a report, not a
  crash; pipe it or `|| true`.
- **Waiting for the column header is not waiting for data.** `MODEL` is drawn
  while the table still says `Scanning for sessions…`. Poll for a value.

## Troubleshooting

- **Table shows `Scanning for sessions…` forever**: the fixture is missing or
  `$HOME` never reached the app. Re-run `driver.sh fixture`, then check
  `driver.sh headless` reports `2 session(s)`.
- **`TIMEOUT waiting for: <text>`**: the driver dumps the last 25 lines of the
  screen with it. Usually the marker moved (shortened model name, renamed
  panel), not a hang.
- **Rows appear with no model, cost or tokens**: that row is a process-table
  row for a live agent outside the fixture, or its transcript lives in a
  `$CODEX_HOME`/`$CLAUDE_CONFIG_DIR` cctop is not reading. `doctor` lists every
  root it walks.
- **`no matches found` from tmux**: the private server is gone (`down` kills
  it). Run `up` again.
