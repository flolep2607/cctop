# Source

Mirrored from <https://github.com/Helvesec/rmux> (`README.md`, `CHANGELOG.md` and `docs/`) at
`1f4571e` on 2026-08-26 by `docs/rmux/pull.sh`.

rmux is the multiplexer cctop hands every tab's agent to, and whose
`web-share` puts an agent's terminal in a browser. cctop meets it as a command
surface — see `src/rmux.rs`.

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
