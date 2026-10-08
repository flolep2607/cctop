# Contributing to cctop

Thanks for helping improve cctop. Please keep changes focused and include tests
when behaviour changes.

## Development

Build and run the test suite with Rust 1.88 or newer (the code uses let-chains):

```bash
cargo test
cargo clippy --all-targets
```

Before opening a pull request, run the whole gate. CI sets
`RUSTFLAGS: -D warnings`, so clippy output that looks advisory locally is a
build failure there:

```bash
# -D warnings is set in .cargo/config.toml
cargo fmt --all --check
cargo clippy --all-targets
cargo test
cargo publish --dry-run --workspace --allow-dirty   # what `verify / package` runs
```

### While iterating, run the tests that could have broken

`cargo test --all-targets` is about 1500 tests and takes 58 seconds of wall time for
about 11 seconds of CPU — it spends most of its life waiting on `thread::sleep`,
real ptys and real subprocesses. Two of those tests assert wall-clock margins
(`hook::tests::advice_is_kept_only_if_it_beat_the_deadline`,
`ui::tabs::tests::a_tab_asks_for_attention_only_when_it_has_something_you_cannot_see`),
so running the full suite concurrently with other builds makes it fail for
reasons that have nothing to do with your change.

The first run after a build can also be several times slower than the next
(#141): 40 s rather than 10, with a handful of tests taking seconds that take
a fraction of one on a rerun. What is known of the cause:

- `recall::tests::recall_finds_the_passage_and_skips_the_caller` used to load
  the fetched 30 MB embedding model from `~/.cache/cctop` — cold off the disk on
  a first run, and only on a machine that had fetched one. Under test,
  `config::CACHE_DIR` is now a temporary directory of the process's own, so no
  test reads or writes the real cache.
- `advise::tests::the_ledger_stays_small` takes an `flock` per write and polls
  for it every millisecond. A child forked by another test in the same binary
  holds an inherited flock until it execs, so under a parallel run with many
  forks that wait can add up.
- The `chat::tests::an_opencode_*` tests build sqlite fixtures; the first one
  pays for sqlite's first touch.

So a slow first run on its own is not a regression: compare it against a second
run of the same command before blaming any one test.

`tools/targeted-test.sh` maps what you changed to the tests worth running:

```bash
./tools/targeted-test.sh                           # diff against main, run what it names
./tools/targeted-test.sh crates/ui/src/filter.rs   # or name the files yourself
./tools/targeted-test.sh --all                     # force the full suite
```

A test's module path is its file's module path inside its own crate, so
`crates/ui/src/filter.rs` runs `filter::` in `-p cctop-ui`, and `src/cli.rs`
runs `cli::` in the binary, `-p cctop`. A crate root (`lib.rs`, `main.rs`) runs
that crate's whole suite; `crates/core/build.rs` or anything non-Rust runs
everything. Only the changed file's own crate is tested, not the crates built
on it — that is what the full run at the end is for. Use it while you work and
the full suite once, at the end, on its own.

## Layout

cctop is one binary built from four crates, so that an edit recompiles only
what sits above it:

| Crate | Where | What |
| --- | --- | --- |
| `cctop` | `src/` | `main`, the command line, and the commands nothing else calls (`doctor`, `why`, `wait`, `mcp`, `recall`) |
| `cctop-ui` | `crates/ui/` | the terminal dashboard |
| `cctop-serve` | `crates/serve/` | the web server |
| `cctop-core` | `crates/core/` | sessions and their parsers, the cache, pricing, config, processes, hooks, rmux, the shim, and what both faces read and do to a session (the conversation, report, export, handoff, actions) |

The dashboard and the server each depend on core and not on each other, and
the binary depends on all three; the root `Cargo.toml` says why each boundary
is where it is. Nothing in core reaches up: something both faces need belongs
in core. The dashboard starts a server to share its table through a function
the binary hands it (`serve_for_dashboard` in `src/main.rs`).

Core's guards that keep a test off the real machine — no bell on stdout, no
copy to your clipboard, no write over your saved preferences, a runtime
directory of the test's own — ask `cctop_core::under_test()`, and the fixtures
other crates' tests use are `cfg(any(test, feature = "test-support"))`.
`cfg(test)` is only true for core's own tests, so the other crates turn the
feature on from their dev-dependencies. A release build never has it; a binary
built by `cargo test` does, and its `main` switches the guards off before
anything else runs, so it behaves as cctop.

## The `debug` feature

`crates/serve/src/debug.rs` is behind `#[cfg(feature = "debug")]`, which is off by
default and never enabled for a release — so an ordinary build contains neither
the routes nor the strings that name them. That is the condition for having them:
a debug surface reachable in a shipped binary is a debug surface somebody else
can reach, and the serve token is one link away from anyone you shared a page
with.

Three things live behind it, all reachable only from a build that asked:

- `/api/debug/state` and `/api/debug/why` — what the server is holding, and how
  it decided which sessions are running. Both are answerable from outside
  (`cctop -j`, `cctop why`), but not from the serving process itself, which is
  where a disagreement between the page and the terminal would show.
- `/api/debug/fault` — makes subsequent `/api/` responses fail in a chosen way
  (`502`, `slow`, `html`, `empty`). The pages have to survive a tunnel whose far
  end has gone and a proxy answering HTML where JSON was asked for, and neither
  can be produced on demand by a correct server.
- `/api/debug/log?level=io` — turns `CCTOP_LOG` on for a server that was not
  started with it.

```bash
cargo run --features debug -- serve --no-token
```

Because the feature is off by default, `cargo clippy --all-targets` and
`cargo test --all-targets` never compile that module — a change to
`crates/serve/src/lib.rs` that breaks the debug routes is green, and nothing finds out
until someone runs it by hand. So `verify` runs both commands a second time
with `--features debug`, and a change that touches those routes should pass both
locally:

```bash
# -D warnings is set in .cargo/config.toml
cargo clippy --all-targets --features debug
cargo test --all-targets --features debug
```

The tests do not exercise the fault routes — arming `slow` sleeps for 30
seconds by design — so the extra run is a compile check, not a timing one. If you
add tests here, keep them off `slow`, or the suite grows half a minute per case.

## cctop is Linux-only

There is one platform, and it is Linux (including WSL). macOS and Windows were
supported once and are not any more: `verify` builds and tests on Linux alone,
and a release ships two statically linked musl archives, x86_64 and aarch64.

Write for Linux directly. A `#[cfg(unix)]` or `#[cfg(target_os = "linux")]`
gate is noise around code that has no other target to be conditional against,
and a stub standing in for a platform that is no longer built is dead code,
which `-D warnings` rejects.

## `cctop hook` must never break the session it watches

`cctop hook` runs inside someone's coding session, many times a minute. Claude
Code reads its exit code as a *decision*: non-zero blocks the tool call and
feeds stderr back to the model. So it exits 0 always, writes nothing to stdout,
and returns inside a deadline — by construction, not by care. See the module
docs in `crates/core/src/hook.rs`.

A hook that fell through to clap would exit non-zero on every fire, so the
`hook` dispatch in `main.rs` is never gated behind anything.

## Conventions

- **`ponytail:` comments** mark a deliberate, documented limit — a thing this
  code knowingly does not do. They are not TODOs and do not want fixing without
  a reason.
- **Comments say why, not what.** The prose in this codebase explains the
  decision behind a line; match that rather than narrating the syntax.
- **Doc comments carry the reasoning** for anything a reader would otherwise
  have to reconstruct — especially where two plausible designs existed.

## Design notes

A few things that are less obvious from the code:

- **Token dedup.** Streaming writes the same `requestId` repeatedly with growing
  counts. Only the last entry per request is counted; summing them all inflates
  totals several-fold.
- **Cache keys carry a pricing generation.** Cached entries hold *computed*
  costs, so a refreshed rate table must invalidate them just as an appended
  transcript does. Without this, sessions priced before the table loaded report
  `$0.00` forever — their transcripts never change again.
- **The cache version is derived, not written.** `build.rs` hashes the shape of
  the serialised types, so adding a field to `SessionData` invalidates stale
  entries without anyone remembering to bump a number.
- **Threads are excluded from process matching.** Threads share their process's
  command line, so every one of them matches the same session and competes to be
  picked as the root — nondeterministically. The winner reports its own CPU and
  no children.
- **Tail reads.** Context usage and last-tool come from seeking backwards from
  EOF, so a live 50 MB transcript costs one 64 KB read per refresh, not a
  reparse.
- **Ghost subagents.** Claude Code purges old subagent transcripts but keeps the
  `tool_use`/`tool_result` pair in the parent. Those rows are reconstructed and
  marked `◌`, with `—` rather than `0` for figures that can no longer be
  measured.
- **Collisions compare repository roots, not directories.** A linked worktree
  carries its own `.git`, so two agents in two worktrees do not collide while
  two in one checkout do. Comparing directories gets both backwards.
- **Remote rows are inert.** A row from `--host` carries `Session::remote`, and
  every action that signals a process, deletes a file, opens a pty or reads a
  git directory guards on it — all of those are about *this* filesystem.

## Releasing

Bump the root version — `version` under `[workspace.package]` in the root
`Cargo.toml`, which is the `cctop` binary's and becomes the tag — and push that
commit, with `Cargo.lock`, to `main`. GitHub Actions derives the matching
`v<version>` tag, creates the GitHub release, builds the Linux archives, and
publishes to crates.io. **The version bump is the release** — there is no
separate confirmation step, and `cargo publish` to crates.io cannot be undone.
Do not create a release tag by hand for a normal version bump.

The three internal crates, `cctop-core`, `cctop-serve` and `cctop-ui`, each
have their own `version` in `crates/*/Cargo.toml`, and the root pins each with
`=` under `[workspace.dependencies]`. A release bumps one only if it changed
since the last `v*` tag, which is any of:

1. **its files changed** — anything under its directory besides its own
   `version` line;
2. **its packaged manifest changed** without that — a `[workspace.dependencies]`
   entry it uses was bumped, or a `[workspace.package]` field it inherits
   (`edition`, `rust-version`, `license`…);
3. **an internal crate it depends on was bumped.** The `=` pins make this
   follow: a published `cctop-ui@0.28.3` requires `cctop-core =0.28.3`, so a
   `cctop` that wants core 0.28.4 beside it resolves two cores and `cargo
   install cctop` fails on mismatched types.

So a change to `crates/ui` alone bumps `cctop-ui` and `cctop`, one to
`crates/serve` alone bumps `cctop-serve` and `cctop`, one to `crates/core`
bumps all four, and one to the binary's own `src/` bumps `cctop` alone. Each
bump is the crate's `version` line, its `=` pin, and `Cargo.lock`.

`tools/bump.sh <version>` makes those edits for you — the root to the version
given, every crate the rules name up one patch, the lock refreshed — but it is
optional, and a bump by hand is as good. Either way:

- **`verify / release-plan`** (`tools/release-plan.sh check`) fails a release
  commit — one whose root version differs from the last tag's — naming each
  crate that changed but kept its version, and each one bumped with nothing
  changed. On any other commit it passes with a notice, since a feature branch
  changes crates without bumping them. Run it locally after committing.
- **`publish-crate`** asks the crates.io index which crates are missing at
  their current version and publishes only those, dependency order first; the
  rest are skipped with a notice. A run that failed part-way can be re-run
  as it is.

**Name the pull request after what the release contains.** The notes GitHub
generates are one line per merged PR, and `cctop --update` prints those lines to
everyone who updates — so the PR title is the release note, and `chore: release
0.7.4` tells a user nothing. `CLAUDE.md` has the full rule and the shapes to
avoid; the short version is to write the sentence you would want someone three
versions behind to read.

Notes are fetched when someone updates rather than when the release is cut, so a
note that landed badly is still worth fixing: edit the release body on GitHub and
everyone who has not updated yet sees the better one.

## Refreshing the screenshots

`docs/assets/` holds real captures, not mock-ups. Regenerate them after any
change to the table's columns or the panels:

```bash
cargo build --release
R="--redact YourCompany=Example Inc"
python3 docs/assets/shot.py docs/assets/dashboard.png --size 146x30 $R
python3 docs/assets/shot.py docs/assets/context.png --keys 'Tab*7' --size 146x36 $R
python3 docs/assets/shot.py docs/assets/demo.gif --record docs/assets/demo.cast \
        --size 128x30 --scale 1 $R
```

The script drives a real cctop through tmux and rasterises the captured screen
itself, so the images carry whatever is on the machine that made them.

**It scrubs email addresses unconditionally**, because cctop reads the signed-in
account out of each harness's config and prints it in the Info panel — an
address the person taking the screenshot never chose to publish and would not
think to look for. `--redact` adds literals (an employer, a client's project
name). Scrubbing happens on the parsed grid, so a replacement of a different
length cannot shift a column: it is padded or truncated to the same width.

Nothing else is scrubbed. Session titles, working directories and real spend
figures all appear as they are — look at what is in frame before committing.

Three traps the script guards, each of which produced a wrong picture first:

- a multiplexer session named `cctop-*` is adopted by cctop as one of its own
  tabs, so it attaches to the terminal it is running in. cctop drives rmux, and
  the script drives tmux, so the two no longer collide — the guard stays because
  the naming rule is what made it a trap, not which daemon held the session;
- preferring `target/release` over `target/debug` captures whichever is *older*,
  which once shipped a screenshot missing two columns added that afternoon;
- an SVG would be smaller and sharper, but cctop draws sparklines with eight-dot
  braille (U+2840+) and DejaVu Sans Mono covers only the six-dot block, so those
  cells become tofu on any reader whose font agrees. Rasterising pins them;
- DejaVu Sans Mono is a wide face, and at its natural line height the cell is
  1.93:1 against the ~2.2 terminals use — every row looks vertically squashed.
  The cell is therefore 1.35× the font size, and the ten box and block glyphs
  (`─━│┊╭╮╯╰█░`) are drawn as geometry rather than text, because a font's box
  characters only span its *own* natural line height and would otherwise leave
  the panel borders visibly gapped.

## Agents' git hooks

`tools/git-hooks` holds three hooks that act only inside a Claude Code session
(`CLAUDECODE=1`) and leave a person's commits alone: one adds the co-author
line to an agent's commit, the other two push each commit (and each merge) in the background on a
branch that already tracks the remote (never `main`, never mid-rebase), so an
agent's draft PR is always current. Enable them once per clone:

```bash
git config core.hooksPath tools/git-hooks
```
