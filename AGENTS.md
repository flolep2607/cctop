# Working on cctop

The instructions for this repository live in [`CLAUDE.md`](CLAUDE.md), and they
apply to any agent working here — read that file first.

It exists under that name because Claude Code loads it automatically. Nothing in
it is specific to Claude: it covers how to avoid colliding with the other agents
working in this checkout, how to verify the way CI does, the cross-platform traps
that have already broken releases, why `cctop hook` must never fail, and how to
title a pull request so the release note it becomes is worth reading.

`CONTRIBUTING.md` covers the parts aimed at a person: the layout of the crate,
the release process, and how the screenshots are regenerated.
