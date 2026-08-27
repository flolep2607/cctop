---
name: run-cctop
description: Build, run, drive, test and screenshot cctop — the terminal dashboard for AI coding agent sessions — including its browser interface (`cctop serve`, the session report, the conversation view). Use when asked to start cctop, launch its TUI, screenshot it, open or exercise its web pages, add or verify a feature, reproduce a rendering bug, run its tests, or check the release gate.
---

cctop is a Rust TUI that watches *other* agents' sessions, so running it needs
two things a clean machine lacks: sessions to show, and a terminal to draw in.
Two drivers sit beside this file: **`driver.sh`** for the TUI and **`web.sh`**
for the browser interface. Neither can see what the other tests.

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

## Run (the browser side)

`driver.sh` drives the TUI through tmux and **cannot see the web interface at
all**. The dashboard page, the session report, the conversation view and the
action routes are a second interface with its own bugs, and `web.sh` is its
driver. Same fixture home, so it never serves the operator's real sessions.

```bash
.claude/skills/run-cctop/web.sh smoke      # serve, shoot three pages, tear down
```

Step by step:

```bash
web.sh chat                                 # give the fixture a conversation worth rendering
web.sh serve                                # start it; prints the URL and waits for a row
web.sh ids                                  # session ids, for building /session/<id>
web.sh api /api/sessions                    # any JSON route, pretty-printed
web.sh shot home /                          # screenshot + the text it rendered
web.sh shot rep "/session/$ID"
web.sh shot dead "/session/$ID" --dead       # every /api/** answers 502 HTML
web.sh down
```

| command | what it does |
|---|---|
| `serve [--token\|--tunnel]` | `cctop serve` against the fixture; waits for the table to have a row, not just for the port to answer |
| `chat` | appends turns to the fixture transcript covering markdown, a table, fenced code, a tool call, a slash command and a reminder — the bare fixture is two lines of plain text and exercises none of the view |
| `shot <name> [path] [--dead]` | PNG **and** the rendered text into `$SHOTS`; exits non-zero on a JS exception |
| `--dead` | answers every `/api/**` with a Cloudflare 502 page — what a trycloudflare tunnel serves once the cctop behind it is gone |
| `ids` / `api` / `url` | the small things every recipe needs |

**Read the `.txt`, not the `.png`.** A screenshot says something is there; the
text says what, and it is what a transcript can carry.

### Reaching the TUI's own server

`B` in the TUI serves the same page without leaving the dashboard — so a change
to the panel is `driver.sh keys B`, and a change to the *page* is `web.sh`.

## Asking cctop what it decided

```bash
cctop why                 # every agent process, and the session it was matched to
cctop why <session-id>    # that session: is it running, and what decided it
cctop doctor              # where sessions are read from, and what is missing
cctop -j                  # every row as JSON
```

`why` is the one that is not obvious. Every other output is a *verdict* — the
table shows a dot, `-j` shows `"running": false` — and says nothing about how it
was reached, so a row that is wrong is a row with nothing to argue with. `why`
prints the reasoning: the pid, the session it was given to, and which of the
four rules gave it (a `--resume` id, a `--session` id, a window title, or the
working directory). The reason is recorded in `proc::collect` where the decision
is made, so it cannot drift from the logic.

The case it exists for: `claude --resume X` **forks**. The agent runs on a new
transcript that records X as the id it came from, so the id on the command line
belongs to a conversation that stopped. Reading it literally marks the dead
conversation as working and the live one as stopped. `why` says
`forwarded to the transcript it forked into` when that has happened.

## Reading a transcript nobody documented

```bash
transcript.py fields <file>       # every key path, with counts and first line
transcript.py find <file> <text>  # which key path holds that value
transcript.py types <file>        # record types, and how many of each
transcript.py diff <a> <b>        # what one file has that the other does not
```

Seven harnesses, none of which documents its JSONL, and the recurring question
is always "which key holds this?". `fields` is how `session_id` was found — the
field recording the id a resumed session was launched from, distinct from
`sessionId`, and the whole reason a forked session can be matched to its
process at all.

## Prefer a render test over a screenshot

For anything drawn by the TUI — a modal, a panel, a column — a `TestBackend`
render test is faster, deterministic, and says what is actually in the buffer.
There are several in `src/ui/mod.rs`; the pattern is:

```rust
let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("backend");
terminal.draw(|frame| layout = render::draw(frame, &mut app)).expect("draw");
let screen: String = terminal.backend().buffer().content()
    .iter().map(|cell| cell.symbol()).collect();
assert!(screen.contains("…"));
```

This is not a preference. A modal bug that three rounds of `driver.sh shot`
could not explain — escape sequences in a cell, and a box sized by line count
while its paragraph wrapped — was obvious in the first buffer dump, because
`capture-pane` shows what the *terminal* made of the output while the buffer
shows what cctop put there.

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

## The multiplexer

cctop drives **rmux**, not tmux (since 0.8 — `src/rmux.rs`). tmux still appears
here because the *driver* uses it as a terminal to run cctop in; the two are
unrelated, and cctop cannot see the driver's tmux server at all.

- **Its tests share one machine-wide daemon.** `rmux::test_lock()` serialises
  them, and a test that kills the last session must wait for it to be gone
  before releasing the lock — killing the last one stops the server, and the
  next test's `new-session` then reaches a socket mid-shutdown and fails. That
  race is why an unrelated test goes red once in five runs.
- **Check rmux's behaviour, don't infer it from tmux.** They differ in ways that
  compile fine: `list-panes -t =NAME` is a parse error under rmux while every
  other target takes the `=`, and a bare name prefix-matches. `rmux -L probe
  new-session -d -s x -- sleep 60` on a private socket is a cheap way to ask.
- **An rmux pane exports five env vars** — `RMUX`, `RMUX_PANE`, `TMUX`,
  `TMUX_PANE`, `TMUX_PROGRAM` — and `new-session -A` refuses to nest under any
  of them. `src/shim.rs` strips all five.
- `docs/rmux/` mirrors rmux's own documentation; `docs/rmux/pull.sh` refreshes
  it. Read it before guessing at a flag.

## Editing hazards

- **`cargo fmt` rewraps lines, so a scripted `str.replace` silently no-ops.**
  Always assert the replacement happened. An edit that quietly did nothing sent
  a whole debugging session after the wrong cause.
- **Never run `cargo publish`**, dry-run included. Run the rest of the gate and
  stop.
- The branch may carry a `[patch.crates-io]` block pinning `rmux-sdk` and
  `ratatui-rmux` to a fork. While it is there the crate cannot be published —
  that is deliberate, and `verify / package` going red is the signal.

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
