<!--
The title is the release note. `cctop --update` shows it, alone, to everyone
who updates: say what changed for a user, with no `fix:`/`feat:` prefix and no
version, and keep the point in the first ~60 characters (CLAUDE.md, "A pull
request title is a release note").
-->

## What changed and why



## Checked by hand

<!-- TUI via .claude/skills/run-cctop/driver.sh, web in both themes and at
phone width, a real remote, … or "nothing beyond the tests" and why. -->

## Gate

<!-- Tick [x] what you ran, mark [-] what does not apply to this change
(e.g. no web/ change), and leave [ ] only for what is still to do. -->

- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets`, `cargo test`
- [ ] Snapshot diffs looked at, and the changed `.snap` files committed
- [ ] `web/` changed → `npm run build` and the rebuilt `crates/serve/src/assets/app/index.html` committed
- [ ] Docs updated where behaviour changed

Fixes #
