# Source

Mirrored from <https://github.com/Helvesec/rmux> (`README.md`, `CHANGELOG.md` and `docs/`) at
`1f4571e` on 2026-08-26 by `docs/rmux/pull.sh`.

rmux is the multiplexer cctop hands every tab's agent to, and whose
`web-share` puts an agent's terminal in a browser. Its server is built into
cctop, from the `cctop-rmux-*` crates published off the fork at
<https://github.com/flolep2607/rmux>, and cctop talks to it over its protocol —
see `crates/core/src/mux.rs` and `crates/core/src/rmux.rs`. Nobody has to
install rmux for cctop. These pages describe upstream's own `rmux` command line,
which cctop does not ship: `cctop mux attach`, `ls` and `kill-session` are the
part of it cctop exposes, on cctop's own socket.

Two things this drops. The artwork: `docs/` is mostly SVG sidebar and wordmark
files, which outweigh the prose and say nothing about a command. And the
translations under `docs/i18n/`, which are the README again in three more
languages.

One thing it cannot take. <https://rmux.io/docs/> is the fuller documentation —
get-started, CLI, API, examples — and it is an interactive site that serves its
prose only as rendered HTML around a playground, with no markdown behind a page.
`docs/man/rmux.1` is the CLI reference that is written down, and `rmux
<command> --help` is the one that ships with the binary.

Upstream's own licence applies to everything in this directory.
