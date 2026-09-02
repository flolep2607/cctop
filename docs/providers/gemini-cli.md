# Gemini CLI

[← all harnesses](README.md) · parser: [`src/session/gemini.rs`](../../src/session/gemini.rs)

Gemini records a per-turn token breakdown and never a cost, so its money column
is inferred from published rates. Its chats also come in two on-disk shapes
depending on the release that wrote them, and the parser reads both as one.

## Where it lives

`$GEMINI_DIR/tmp/<project>/chats/session-*.json{,l}`, defaulting to `~/.gemini`
(`config::GEMINI_HOME`). The `tmp/` reads like something disposable; it is where
the transcripts actually live.

Discovery descends exactly two levels — project, then `chats/` — rather than
globbing. The sibling `tool-outputs/` holds a file per tool call and dwarfs the
transcripts beside it, so a recursive walk would spend most of its time in a
directory with nothing in it cctop wants.

The working directory comes from a `.project_root` file written beside
`chats/`. Older subtrees are named for a *hash* of that path with no marker to
undo it; those keep an empty label, because a hash rendered in the LABEL column
looks like a path and is not one.

## Identity, and why it is the filename

`Session::session_id` is the filename stem, not the `sessionId` inside the file.
Resuming a chat opens a **new file under the same `sessionId`**, and the
segments are disjoint rather than cumulative — so keying rows on that id would
collapse them onto each other and lose every earlier segment's usage. The
filename is unique and is what Gemini itself indexes by.

The start time is parsed straight out of that stem
(`session-2026-05-14T17-34-79709c93` → minute precision), which is all the table
sorts on. Reading it from the file instead would mean deserialising a
multi-megabyte JSON conversation during discovery, since the single-object shape
has no header to stop at.

## The two formats

- **Older:** one JSON object holding the whole conversation, with a `messages`
  array.
- **Newer:** JSONL — a header line, one line per message, and `{"$set": …}`
  patches revising header fields in place.

`for_each_record` unwraps `$set` patches and hands them over as plain objects.
Every field a patch revises is one the header already declares, so a caller that
simply takes the last value it sees ends up with the same header the JSON form
states outright.

## Tokens, verified

A `gemini` record's `tokens` object carries `input`, `cached`, `output`,
`thoughts`, `tool` and `total`. Checked against 605 recorded turns:

- `total` equals `input + output + thoughts + tool`, exactly, and
- `cached` is **never** among the addends — it is the part of `input` that was
  served from context cache, not an addition to it.

So the full-rate input is the uncached remainder, which is the same split Codex
reports. `thoughts` is billed as output, which is how Google bills it and is the
larger half of a reasoning turn on Gemini 3. The `tool` bucket has no rate of
its own and reads zero on every turn seen so far, so it is left inside `total`
rather than folded into a bucket that would be charged.

## Cost

Estimated, through `pricing::resolve_codex`. The name is Codex's; the *shape* —
flat input, cached input, output, no cache-write tier — is Gemini's billing
model too, and a duplicate resolver under a different name would buy nothing.

A session on a bundled Code Assist tier still shows an estimate: nothing in the
transcript says which tier it ran on. See
[What the cost figures mean](../costs.md).

## What cannot be extracted

**Context is `─`.** There is no `extract_context` for Gemini —
`loader::read_tail` returns `None` for it. The transcript records what a turn
consumed but not how full the window was afterwards, and the two are not the
same number.

**No live process.** Gemini takes no session id on its command line, so there is
nothing to tie a PID to a chat file; Gemini is not among the providers
`proc::could_be_agent` recognises at all. Rows always read as stopped and the
CPU and memory columns stay blank rather than borrowing another session's
figures. `Session::resume_argv` is `None` for the same reason.

Tool outcomes *are* recorded — a call's `status == "error"` — so `ERR%` is real
here, unlike Pi's.

The title is the `summary` field, which is what Gemini's own `update_topic` tool
writes; it is the closest thing to a session name the format has.

`--install-hooks` writes `$GEMINI_DIR/settings.json` in the same nested shape
Claude uses, so a live Gemini session can report the states its file cannot.
