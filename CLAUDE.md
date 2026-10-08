# Working on cctop

## Work in a worktree, not in the checkout

Several agents run on this repository at once. They collide: a test body in
`src/ui/mod.rs` was overwritten twice in one afternoon, and `src/hook.rs` was
left calling a function that did not exist yet while another session was
mid-edit. Nothing was lost, but only because someone was watching.

So take a worktree of your own before you edit anything:

```bash
git worktree add .claude/worktrees/agent-$ID -b worktree-agent-$ID
```

`$ID` is anything unique to you. That is the existing convention — `git worktree
list` shows the ones already there — and the branches merge back normally.

The cost of skipping it is not a merge conflict, which git would at least
announce. It is a silent overwrite of someone else's uncommitted work.

If you are already editing the main checkout and another session is too, say so
rather than racing: whoever is further along should finish first.

## Verify the way CI does

CI sets `RUSTFLAGS: -D warnings`, so a warning is a build failure. Clippy output
that looks advisory locally is fatal there. `.cargo/config.toml` sets the same
flag for every cargo command in the checkout, so there is nothing to export —
and exporting or unsetting `RUSTFLAGS` by hand rebuilds every dependency. The Rust version is pinned in
`rust-toolchain.toml`, which rustup and CI both read, so local clippy is CI's
clippy — moving to a newer Rust is a pull request that changes that one line.
Run the whole gate before pushing:

```bash
# -D warnings is set in .cargo/config.toml
cargo fmt --all --check
cargo clippy --all-targets
cargo test
cargo publish --dry-run --workspace --allow-dirty   # what `verify / package` runs
```

Whole screens are pinned as snapshots (`crates/ui/src/snapshot.rs`, with the `.snap`
files beside it in `crates/ui/src/snapshots/`). A change to what the TUI draws fails
them on purpose. Look at the diff, and if the new frame is the one you meant,
accept it with `cargo insta review` (needs `cargo install cargo-insta`) or with
`INSTA_UPDATE=always cargo test`, and commit the `.snap` files that changed. CI
only compares against the committed files and never writes new ones.

## Run only the tests your change can break

`cargo test --workspace --all-targets` is about 1570 tests, and on a warm build
it takes about 5 seconds of wall time for 6 seconds of CPU (6 cores, already
loaded to about 4 by other lanes, median of seven runs). It used to be a minute
of mostly `thread::sleep`; the sleeps are now waits on the condition itself
(`cctop_core::test_wait`), so what is left is the real ptys and subprocesses,
and the slowest binaries take about a second each. No test asserts a tight
wall-clock margin any more: the few that check `elapsed()` bound something
meant to be instant by fifty times what it needs. A full-suite failure on a busy
machine is still not evidence until you have re-run it alone — fork-heavy tests
share one process, and a race between them is not your change.

While working, run `tools/targeted-test.sh`, which maps changed files to the
tests that cover them (`crates/ui/src/filter.rs` → `-p cctop-ui filter::`), and
saves the full suite for one final run on its own. `CONTRIBUTING.md` has the
details.

## cctop is four crates

The binary is at the root; `cctop-core`, `cctop-serve` and `cctop-ui` are under
`crates/`. The UI and the server each depend on core and not on each other. The
root `Cargo.toml` draws the graph and says why each boundary is where it is.
The point is the rebuild: an edit to the UI recompiles the UI and the binary,
not the server and not the parsers.

- From a crate above core, a module of core is `cctop_core::x`. If the item you
  want is `pub(crate)`, widen it to `pub` rather than copying it.
- Nothing in core may reach up. Something both faces need goes down into core.
- A guard in core that keeps tests off the real machine — the bell, the
  clipboard, saved preferences, the runtime directory — asks
  `crate::under_test()`, not `cfg!(test)`, which is false while the UI's or the
  server's tests run. A fixture those tests use is
  `cfg(any(test, feature = "test-support"))`; their dev-dependencies turn the
  feature on. See `under_test` for why the binary still behaves as cctop.
- Each crate has its own version. The root `cctop`'s is the release version
  (`[workspace.package]` in the root `Cargo.toml`); cctop-core, -serve and -ui
  carry theirs in `crates/*/Cargo.toml`, with `=` pins on them in the root's
  `[workspace.dependencies]`. A release bumps the root, and an internal crate
  only if it changed since the last tag — its files, its packaged manifest, or
  an internal crate it depends on (so a core change bumps all four). Each bump
  is its `version` line, its pin and `Cargo.lock`; `tools/bump.sh` does it, and
  `verify / release-plan` fails a release that got it wrong.

## cctop is Linux-only

There is one platform, and it is Linux. macOS and Windows were supported once
and are not any more: `verify` builds and tests on Linux alone, and a release
ships two statically linked musl archives, x86_64 and aarch64.

So write for Linux directly. A `#[cfg(unix)]` or `#[cfg(target_os = "linux")]`
gate is noise around code that has no other target to be conditional against,
and a stub standing in for a platform that is no longer built is dead code,
which `-D warnings` rejects.

## `cctop hook` must never break the session it watches

`cctop hook` runs inside someone's coding session, many times a minute. Claude
Code reads its exit code as a *decision*: non-zero blocks the tool call and
feeds stderr back to the model. So it exits 0 always, writes nothing to stdout
that could read as a decision, and returns inside a deadline — by construction,
not by care. The one thing it may print is the opt-in `warn_agents` context,
which carries no decision; see "The one answer that is not silence" in the
module docs of `crates/core/src/hook.rs`.

A hook that fell through to clap would exit non-zero on every fire, so the
`hook` dispatch in `main.rs` is never gated behind anything.

One command may decide, and it is not `cctop hook`: `cctop yolo-hook`,
installed beside it for Claude Code's `PermissionRequest` alone. It prints the
`allow` answer for a session YOLO was switched on for, and only when the
payload's `session_id` and the agent process that spawned it (pid and start
time) both match what was recorded at the switch; for anything else it is the
same silence, exit 0 and deadline as `cctop hook`. It never denies. Keep the
exception that narrow — a new decision belongs in its own command with its own
match, not in `cctop hook` — and see `hook::yolo_hook` and the module docs of
`crates/core/src/yolo.rs` for why it is safe.

## A pull request title is a release note

Whatever you call the PR is what users read. `release.yml` asks GitHub to
generate the release notes, which is one line per merged PR — `* <title> by
@someone in <url>` — and `cctop --update` prints those lines to whoever updates,
having stripped the attribution and the URL. The title is all that survives, so
it is the whole note.

Write it as the sentence you would want someone three versions behind to read:

```
Codex accounts per subscription, and a tab you can close without losing your place
Fix the login hint for a named account, and add a run skill that drives the TUI
```

Both of those are real titles, and both read correctly on an updater's screen.
These do not:

- **`chore: release 0.7.4`.** This is what 0.7.4 actually shipped as its only
  release note, on the release that introduced the feature that prints them.
  A release PR's title has to name what the release *contains* — its theme, in
  the user's terms — because the bump is the least interesting thing about it.
- **A type prefix.** `fix:` and `feat:` belong on commits, where the audience is
  someone reading `git log`. On an updater's screen they spend width telling a
  user what a maintainer would have wanted to know.
- **A leading version.** `0.7.0: shared tabs` and `release 0.7.3: …` are both in
  this history. The updater prints each note under a version heading and strips
  a version that repeats it, but that strip is a rescue for what is already
  published, not a licence.

Keep the point inside the first sixty characters or so. Long titles are wrapped
to the terminal with a hanging indent rather than truncated, so nothing is lost
— but the first line is what gets read.

A note is fetched when someone updates, not when the release is cut, so a bad
one can be fixed after the fact: edit the release body on GitHub and everyone who
has not updated yet gets the better version. `gh release view v<version> --json
body` shows what they would see now.

## The web UI is a React app in `web/`, committed built

`cctop serve` is one React app: **Vite + React + TypeScript + Tailwind +
shadcn/ui** in `web/`, with a route per page — the table (`/`), a session
(`/session/:id`), the workspace (`/workspace`) and analytics (`/analytics`).

```bash
cd web && npm ci          # Node from web/.nvmrc
npm run dev               # hot reload, proxied to `cctop serve --no-token --port 7778`
npm run build             # writes crates/serve/src/assets/app/index.html — commit it
npm run lint
```

How it ships, and why:

- **One file, everything inlined.** `vite-plugin-singlefile` builds the whole
  app — scripts, styles, fonts — into `crates/serve/src/assets/app/index.html`.
  `crates/serve/build.rs` compresses it (brotli and gzip) when cctop is built,
  and the binary carries only those copies. The content policy loads nothing
  from any URL, and an installed cctop is one binary.
- **The build is committed**, so `cargo install` needs no Node. CI's
  `verify / web` job rebuilds it and fails if it differs — change `web/` and
  run `npm run build` in the same commit.
- **The server's values are fetched** from `/api/config` (`app_config` in
  `crates/serve/src/lib.rs`), token-gated and never cached: token, whether
  actions are allowed, home, version. They stay out of the HTML so the page is
  the same bytes on every run and can be compressed at build time.
  `web/src/lib/config.ts` reads them; every request goes through
  `web/src/lib/api.ts`, which adds the token.
- **Size is paid on every load**, so heavy dependencies need a reason: `wouter`
  rather than react-router, Latin font subsets only.
- **A moved iframe reloads.** The terminal frames (workspace tiles, the session
  page's terminal) must keep their place in the DOM; reorder with CSS `order`.

Check a visual change in both themes and at phone width:
`CCTOP_SCHEME=dark` and `CCTOP_WIDTH=390` in front of `web.sh shot`.

## Conventions

- **`ponytail:` comments** mark a deliberate, documented limit — a thing this
  code knowingly does not do. They are not TODOs and do not want fixing without
  a reason.
- **Comments say why, not what.** The prose in this codebase explains the
  decision behind a line; match that rather than narrating the syntax.
- **Doc comments carry the reasoning** for anything a reader would otherwise
  have to reconstruct — especially where two plausible designs existed.
