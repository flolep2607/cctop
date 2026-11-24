# Antigravity CLI

[← all harnesses](README.md) · parser: [`crates/core/src/session/antigravity.rs`](../../crates/core/src/session/antigravity.rs)

Google's `agy`. Its log is one plain JSONL file per conversation and easy to
read, but that log leaves out the working directory and the title, so cctop
has to get both from a second file. It is also the one harness here that
starts subagents in bulk: a swarm run starts one conversation per worker.

## Where it lives

`~/.gemini/antigravity-cli/`, or `$ANTIGRAVITY_DIR` for the whole directory.
Each conversation is a directory, `brain/<uuid>/`, and its log is
`brain/<uuid>/.system_generated/logs/transcript.jsonl`. When cctop runs as
root, it also reads the same directory under other users' homes.

The same log is also written in pieces under `logs/chunks/transcript/`, and
those pieces concatenate byte for byte to `transcript.jsonl`. So only the whole
file is read. `transcript_full.jsonl` has the same steps with `content`
untruncated, which none of cctop's figures need.

## Where the directory and title come from

The transcript records neither the working directory nor a title. Both come
from `conversation_summaries.db`, which sits beside `brain/`. It is SQLite and
cctop opens it read-only. The `conversation_summaries` table has one row per
conversation: `title`, `workspace_uris` (a JSON list of `file://` URIs, of
which the first local one is used, percent-decoded) and
`parent_conversation_id`.

If the index won't open, the transcripts are still listed, just without a
title, a directory or a parent.

## Subagents fold into their parent

A conversation with a `parent_conversation_id` is not listed as a row of its
own. Its tokens, cost and tool calls count toward the conversation that
started it, at any depth, and each one gets a subagent entry with its own
share. This is the same treatment Claude Code's sidechains get. Listed
separately, a dozen swarm workers would bury the one conversation a person
actually typed into.

So a parent's row is as fresh as the newest of its subagents' transcripts. The
row's `last_active`, and the cache key built from it, use that newest time
rather than the parent's own file.

## The format

One line per step. Each step has `type` (`USER_INPUT`, `PLANNER_RESPONSE`,
`GENERIC`, `SYSTEM_MESSAGE`, …), `status` and `created_at`.

- **A `PLANNER_RESPONSE` is a model turn.** It carries `model`,
  `input_tokens`, `cache_read_tokens`, `output_tokens` and the `tool_calls` it
  made.
- **`cache_read_tokens` is separate from `input_tokens`, not part of it.** The
  prompt is the sum of the two: one turn reads 2,520 input and 11,777 cached,
  right after a turn that read 13,834 input and none cached.
- **Each call is answered in order by a `GENERIC` step.** The call failed when
  that step's `status` is `ERROR` or `INVALID`. A failed edit's diff is kept
  as what the agent attempted, but its lines are not counted as added or
  removed.
- **Arguments are double-encoded and named in PascalCase.** For example,
  `"CommandLine": "\"git status\""`. They are decoded and renamed
  (`CommandLine` → `command`, `Path` → `file_path`, and so on), so a command
  or a diff reads the same as it does for every other harness.

## Cost

Estimated at API rates, as for Gemini CLI. `agy` names a model as an API model
plus an effort level, and `pricing::antigravity_api_model` strips the effort
level to get the name it is billed under. Its orchestrators and swarm worker
have no API name of their own and are priced as the model they run. That
choice is marked `ponytail:` in the code.

## Context

The newest turn's whole prompt, measured against the window LiteLLM gives for
the API model. When LiteLLM doesn't know that model, Gemini's default window
is used.

## What cctop will not do

`delete` returns `Unsupported`. A conversation is its `brain/` directory, its
`conversations/<id>.db`, its row in the summaries index and its subagents'
copies of all three, and `agy` may hold any of them open. Removing only the
transcript would leave `agy` listing a conversation with no log.

There is no live-process match yet: an `agy` process is not tied to its row,
so the row shows no pid. Resume is `agy --conversation <id>`.

> *ponytail:* written against `agy` 1.x as found on disk; Google publishes no
> schema. Unknown fields are ignored and a missing one reads as zero, so a
> format change costs figures, never a wrong row.
