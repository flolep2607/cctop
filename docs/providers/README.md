# What cctop reads, per harness

[← back to the README](../../README.md)

Eight harnesses, eight pages. Each one says where that harness's sessions live,
what shape they are in, which columns cctop can fill from them, and — the part
worth writing down — where the format says something other than what it appears
to say.

These are cctop's own notes, written from the parsers in `crates/core/src/session/`. They
are not a description of the harness: they are a description of what our code
believes about it, which is a narrower and more useful thing. Every claim here
should be traceable to a function you can open.

| | Parser | Where its data lives |
|---|---|---|
| [Claude Code](claude.md) | `crates/core/src/session/claude.rs` | `~/.claude/projects/<slug>/<uuid>.jsonl` |
| [Codex](codex.md) | `crates/core/src/session/codex.rs` | `~/.codex/sessions/**/rollout-*.jsonl` |
| [Cursor](cursor.md) | `crates/core/src/session/cursor.rs` | `~/.cursor/projects/*/agent-transcripts/**/*.jsonl` |
| [Devin](devin.md) | `crates/core/src/session/devin.rs` | `~/.local/share/devin/cli/{sessions.db,transcripts/}` |
| [Gemini CLI](gemini-cli.md) | `crates/core/src/session/gemini.rs` | `~/.gemini/tmp/<project>/chats/session-*.json{,l}` |
| [OpenCode](opencode.md) | `crates/core/src/session/opencode.rs` | `~/.local/share/opencode/opencode*.db` |
| [Pi](pi.md) | `crates/core/src/session/pi.rs` | `~/.pi/agent/sessions/**/*.jsonl` |
| [Windsurf](windsurf.md) | `crates/core/src/session/windsurf.rs` | `<Windsurf User>/workspaceStorage/*/state.vscdb` |

And [adding a harness](adding-a-harness.md), which is the list of places a
ninth one has to be registered before it works — derived from what the eight
already touch, not from a design.

## Why this is not `docs/harnesses/`

[`docs/harnesses/`](../harnesses/) is a **mirror**: seven of these eight
projects' own documentation, pulled wholesale by `pull.sh` and overwritten
wholesale on the next pull. Nothing in it is edited by hand, because the next
refresh would delete the edit — and its value is precisely that it does not
move, so a format change can be read against yesterday's copy of the upstream
prose.

That leaves nowhere to say what *we* do, so this directory says it: one page
per harness, ours, versioned with the parser it describes. The two are meant to
be read together — the mirror tells you what the harness says it writes, and
these pages tell you what cctop found when it went and read it.

## The reason behind each `─`

The README's capability table marks with `─` anything a harness does not
record. That table is deliberately terse; the reason lives here.

| | Cost | Tokens | Context | Tools | Live process |
|---|---|---|---|---|---|
| Claude Code | estimated | ✓ | ✓ full breakdown | ✓ | ✓ |
| Codex | estimated | ✓ | ✓ | ✓ | ✓ |
| OpenCode | reported | ✓ | ✓ | ✓ | ✓ |
| Pi | reported | ✓ | ─ | ✓ | ✓ |
| Gemini CLI | estimated | ✓ | ─ | ✓ | ─ |
| Devin | ─ | ✓ | ✓ | ✓ | ✓ |
| Cursor | ─ | ─ | ─ | ✓ | inferred |
| Windsurf | ─ | ─ | ─ | ✓ | ─ |

Devin is the odd one out in the other direction: it records everything except
the price. Neither its transcript nor its database holds a dollar figure, so its
`$` is a `─` rather than an estimate — and `doctor` skips it in the unpriced-model
check for the same reason, since a blank is not a price of zero.

Three of those cells are conditional rather than absolute, and the pages say so:
OpenCode's context needs LiteLLM to know the model's window, since OpenCode
records no ceiling of its own; Devin's is the same case, its `num_tokens_preceding`
without a window of its own; and Windsurf's tool count is read from a bubble
shape nobody has yet confirmed against a live install.

"Estimated" against "reported" is a policy, not a per-harness accident —
[What the cost figures mean](../costs.md) is where that policy lives, and these
pages only record how each harness happens to land on one side of it.

## A note on the traps

Most of what is on these pages started as a bug. A resume that pointed a live
process at a dead transcript, two thirds of Codex's tool calls rendering with no
arguments, an oversized line that silently dropped a turn's cost. They are
written down because the format made each of them look correct, and the next
person reading the same field will draw the same conclusion unless the page
warns them.
