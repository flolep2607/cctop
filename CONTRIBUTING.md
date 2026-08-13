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
export RUSTFLAGS="-D warnings"
cargo fmt --all --check
cargo clippy --all-targets
cargo test
cargo publish --dry-run --allow-dirty   # what `verify / package` runs
```

## Cross-platform is not optional

`verify` builds on Linux, macOS **and** Windows, and all three gate a release.
The traps, all of which have bitten:

- **Windows has no ptys or unix sockets.** `shim` is `#[cfg(unix)]` with
  `shim_stub.rs` standing in. A stub may only carry what Windows actually
  reaches — an item whose callers are all gated is dead code, which
  `-D warnings` rejects. Adding a courtesy stub breaks the build.
- **A `cfg`'d-out caller makes its callee dead code.** If every caller of a
  function is `#[cfg(unix)]` or `#[cfg(target_os = "linux")]`, the function
  needs the same gate.
- **macOS resolves symlinks.** `tempfile::tempdir()` gives `/var/folders/…`;
  FSEvents reports `/private/var/folders/…`. Canonicalise before comparing a
  path the watcher reported against one you built.
- **Windows filenames reject `|`, `:`, `*`, `?`.** A fixture using them needs
  `#[cfg(unix)]`.
- **Paths in tests.** A transcript written on Unix spells absolute paths with a
  leading `/`, which `Path::is_absolute` calls false on Windows. Compare against
  `std::path::MAIN_SEPARATOR` rather than hard-coding `/`.

## `cctop hook` must never break the session it watches

`cctop hook` runs inside someone's coding session, many times a minute. Claude
Code reads its exit code as a *decision*: non-zero blocks the tool call and
feeds stderr back to the model. So it exits 0 always, writes nothing to stdout,
and returns inside a deadline — by construction, not by care. See the module
docs in `src/hook.rs`.

This is why the `hook` dispatch in `main.rs` is not behind `#[cfg(unix)]`:
`--install-hooks` writes the settings file on any platform, and a hook that fell
through to clap would exit non-zero on every fire.

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

Change the package `version` in `Cargo.toml` and push that commit to `main`.
GitHub Actions derives the matching `v<version>` tag, creates the GitHub
release, builds the platform archives, and publishes the crate. **The version
bump is the release** — there is no separate confirmation step, and
`cargo publish` to crates.io cannot be undone. Do not create a release tag by
hand for a normal version bump.

## Refreshing the screenshots

`docs/assets/*.png` are real captures, not mock-ups. Regenerate them after any
change to the table's columns or the panels:

```bash
cargo build --release
python3 docs/assets/shot.py docs/assets/dashboard.png --size 146x30
python3 docs/assets/shot.py docs/assets/context.png --keys 'Tab*7' --size 146x36
```

The script drives a real cctop through tmux and rasterises the captured screen
itself. Two traps it now guards against, both of which produced a wrong picture
before they were: a tmux session named `cctop-*` gets adopted by cctop as one of
its own tabs, so it attaches to the terminal it is running in; and preferring
`target/release` over `target/debug` silently captures whichever is *older*, in
one case shipping a screenshot missing two columns added that afternoon.

The images carry whatever is on the machine that made them — session titles,
working directories and real spend figures. Check what is in frame before
committing one.
