# Windsurf

[← all harnesses](README.md) · parser: [`src/session/windsurf.rs`](../../src/session/windsurf.rs)

The least legible of the seven, and the only page here with a standing caveat on
its own accuracy. Windsurf is a VS Code fork, so its conversations live inside
the editor's per-workspace settings database, and its accounting lives on
Codeium's servers where nothing local can see it.

## Where it lives

`$WINDSURF_USER_DIR/workspaceStorage/<hash>/state.vscdb`. Without the override,
the User directory follows the platform: `~/Library/Application
Support/Windsurf/User` on macOS, `%APPDATA%\Windsurf\User` on Windows, and the
XDG config directory elsewhere. `$WINDSURF_USER_DIR` names the whole `User`
directory rather than a parent, which is what a portable install actually moves.

The working directory comes from `workspace.json` beside the database, where VS
Code records it as a `file://` URI; it is percent-decoded by hand, because
undoing `%XX` is the entire job and a URI crate would be a dependency for six
lines. A remote or virtual workspace has no local path and keeps an empty label.

## The format

SQLite, opened read-only. `ItemTable` maps a settings key to a JSON blob, and
Cascade parks its conversations in one of those blobs as a list of `tabs`, each
holding the `bubbles` of one conversation. One row per tab: `tabId` for the id
(string or number, both accepted), `chatTitle` for the title.

The stored value is taken as raw bytes, `Text` or `Blob` alike: VS Code writes
some `ItemTable` entries as one and some as the other, and asking rusqlite for a
concrete type fails outright on the wrong one.

## The caveat

> *ponytail:* the parser is written against the documented `ItemTable` layout
> rather than a Windsurf install, so the settings-key list and the bubble field
> names are the part that could be wrong.

Four keys are tried in order — `cascade.chatdata`,
`workbench.panel.aichat.view.aichat.chatdata`, `aiChat.chatdata`, `chat.data` —
and the first that parses into `tabs` wins. Tool calls are read from either
`toolCalls` or `tool_calls`, with a name under either `name` or `toolName`.

Both fail *closed*: an unrecognised key yields no rows, an unrecognised bubble
yields no tool calls. So a wrong guess costs visibility and never produces a
wrong number. If you can read the real key off a live install, add it to
`CHAT_DATA_KEYS`. Until then, the README's `✓` in Windsurf's Tools column is
provisional in a way the other six are not.

## What cannot be extracted

No model, no tokens, no cost, no per-message timestamp, no tool arguments, no
per-call outcome. Windsurf bills in credits server-side and keeps none of it
locally, so `list_sessions` sets `cost_available = false` and `total_cost =
None`: these sessions report cost as *unavailable* rather than as free. Tool
details carry the tool's name and nothing else, because a fabricated argument
line would read as though the blob had recorded one.

**Both timestamps are the database's mtime.** Windsurf stamps neither a start
nor an end on a conversation, so every tab in a workspace shares the file's
time. That is honest about ordering — the most recently used workspace really
did change last — and deliberately says nothing else.

The consequence reaches the cache: `cache::disk_key` returns `None` for
Windsurf, because there is no per-conversation stamp to build an honest key
from, so its rows are never persisted and re-extract together whenever the
workspace file moves. That is cheap, since the blob is small.

## What cctop will not do

`delete` returns `Unsupported` with a message pointing at the editor. Removing
one conversation means rewriting a settings blob inside a live editor's own
database, which is not a thing to do behind a running Windsurf — and reporting
the refusal beats returning success and leaving the row on screen.

There is no resume command, no process to match (Windsurf's agent is the editor
itself), and no hooks: `access.rs` lists a project's `.windsurfrules` and notes
that the global rules live in the settings UI, where nothing can read them.
