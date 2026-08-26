# The harnesses, in their own words

cctop reads what seven different coding agents leave on disk. None of them
promises to keep that shape: a transcript field gets renamed, a session moves
out of `~/.config`, a hook grows a new event, and the first cctop hears of it is
a column that has gone blank.

This directory is each harness's own documentation, mirrored. It is here so a
change can be read against a copy that does not move under you — and so the
answer to "what does Codex actually write into a rollout file?" is a `grep`
rather than seven browser tabs.

| | Mirrored from | Where cctop meets it |
|---|---|---|
| [claude](claude/) | code.claude.com/docs | JSONL transcripts under `~/.claude/projects`, and the hook settings `--install-hooks` writes |
| [codex](codex/) | github.com/openai/codex | rollout files under `$CODEX_HOME/sessions`, accounts in `auth.json` |
| [cursor](cursor/) | cursor.com/docs | its agent sessions, and the Codex server it bundles |
| [gemini-cli](gemini-cli/) | github.com/google-gemini/gemini-cli | its session logs |
| [opencode](opencode/) | github.com/anomalyco/opencode | its session storage |
| [pi](pi/) | github.com/earendil-works/pi | sessions under `$PI_CODING_AGENT_DIR`, default `~/.pi/agent` |
| [windsurf](windsurf/) | docs.windsurf.com | its session logs |

rmux is mirrored the same way and for the same reason, one directory up in
[`docs/rmux`](../rmux/): it is not a harness, but cctop drives it by a command
surface nobody promised to keep.

## Refreshing it

```bash
./docs/harnesses/pull.sh            # all seven
./docs/harnesses/pull.sh claude pi  # just these
```

Each directory is overwritten wholesale, so **nothing here is edited by hand** —
a fix belongs upstream, and a note of our own belongs in `docs/` proper. Every
directory carries a `SOURCE.md` saying where it came from and when, which is how
you tell a stale mirror from a harness that genuinely stopped documenting
something.

Two things the mirror deliberately drops: screenshots and theme galleries, which
outweighed the prose and said nothing about a file format; and every non-English
translation OpenCode ships. Windsurf is the one site that will not serve the
markdown behind a page, so it is reconstructed from its `llms-full.txt` by
[`windsurf-split.py`](windsurf-split.py).

The crate excludes this directory — `cargo publish` has a 10 MB limit and this
is 14 MB of other people's documentation. Each harness's own licence applies to
its directory.
