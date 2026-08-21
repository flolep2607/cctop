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
that looks advisory locally is fatal there. Run the whole gate before pushing:

```bash
export RUSTFLAGS="-D warnings"
cargo fmt --all --check
cargo clippy --all-targets
cargo test
cargo publish --dry-run --allow-dirty   # what `verify / package` runs
```

## Cross-platform is not optional

`verify` builds on Linux, macOS **and** Windows, and all three gate a release.
Two whole platforms broke for 33 commits without anyone noticing, because CI
only ran on `main` and the branch never opened a PR.

The traps, all of which have bitten:

- **Windows has no ptys or unix sockets.** `shim` is `#[cfg(unix)]` with
  `shim_stub.rs` standing in. A stub may only carry what Windows actually
  reaches — an item whose callers are all gated is dead code, which `-D
  warnings` rejects. Adding a courtesy stub breaks the build.
- **A `cfg`'d-out caller makes its callee dead code.** If every caller of a
  function is `#[cfg(unix)]` or `#[cfg(target_os = "linux")]`, the function
  needs the same gate.
- **macOS resolves symlinks.** `tempfile::tempdir()` gives `/var/folders/…`;
  FSEvents reports `/private/var/folders/…`. Canonicalise before comparing a
  path the watcher reported against one you built.
- **Windows filenames reject `|`, `:`, `*`, `?`.** A fixture using them needs
  `#[cfg(unix)]`.

## `cctop hook` must never break the session it watches

`cctop hook` runs inside someone's coding session, many times a minute. Claude
Code reads its exit code as a *decision*: non-zero blocks the tool call and
feeds stderr back to the model. So it exits 0 always, writes nothing to stdout,
and returns inside a deadline — by construction, not by care. See the module
docs in `src/hook.rs`.

This is why the `hook` dispatch in `main.rs` is not behind `#[cfg(unix)]`:
`--install-hooks` writes the settings file on any platform, and a hook that fell
through to clap would exit non-zero on every fire.

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

## Conventions

- **`ponytail:` comments** mark a deliberate, documented limit — a thing this
  code knowingly does not do. They are not TODOs and do not want fixing without
  a reason.
- **Comments say why, not what.** The prose in this codebase explains the
  decision behind a line; match that rather than narrating the syntax.
- **Doc comments carry the reasoning** for anything a reader would otherwise
  have to reconstruct — especially where two plausible designs existed.
