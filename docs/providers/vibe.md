# Mistral Vibe

[← all harnesses](README.md) · parser: [`crates/core/src/session/vibe.rs`](../../crates/core/src/session/vibe.rs)

Vibe has written its sessions two ways, and one home can hold both: the Python
CLI's plain directory, and the newer store the Rust frontend's harness keeps,
which is a content-addressed snapshot plus a journal. Most of this page is about
the second, because what it records is good and where it records it is not
obvious.

## Where it lives

`~/.vibe/logs/session/` (`config::VIBE_SESSIONS_ROOT`). `$VIBE_HOME` moves the
whole `~/.vibe`, which is Vibe's own variable for it, and other homes are found
through `vibe_sessions_roots()`.

Configuration sits in the same tree: `~/.vibe/config.toml` (with a project's
`.vibe/config.toml` over it), `~/.vibe/skills/`, and a project's `AGENTS.md`.

## The format

**`session_<date>_<time>_<id8>/`**, from the Python CLI:

| File | What cctop takes |
|---|---|
| `meta.json` | `session_id`, `start_time`, `end_time`, `environment.working_directory`, `title`, `config.active_model`, and `stats`: `session_prompt_tokens`, `session_completion_tokens`, `tool_calls_failed`, `context_tokens` |
| `messages.jsonl` | the conversation in OpenAI chat shape: string `content`, `reasoning_content`, `tool_calls` whose arguments are a JSON document in a string |

**`unified/<id>/`**, from the newer store (`store_format`
`mistral.vibe.unified-session-store/v1`):

| File | What cctop takes |
|---|---|
| `meta.json` | the same header as above, without `stats` or `config` |
| `CURRENT` | which generation is the snapshot |
| `generations/<n>/manifest.json` | the chunk ids that make up the snapshot's `checkpoint` and `projection_state` |
| `chunks/<sha256>.json` | arrays of items. The checkpoint's are the model's context (`message.role`, typed `content` parts, a tool message's `outcome`); the projection's are the UI's history, where a `model_change` entry names the model |
| `generations/<n>/runtime-state.json` | `session_metadata.active_model`, when one was chosen for the session |
| `generations/<n>/projection-state.json` | the snapshot's envelope: `session.status`, `tokenUsage`, `contextUsage` |
| `journal/<seq>.jsonl` | every record since the snapshot, one per line |

Every session's `data_file` is its `meta.json`, in both layouts: it is the one
file both have, and the directory around it is the session.

## The traps

**The snapshot is only written when a turn ends.** During a turn the journal is
the only live copy, and the journal carries a fresh `set_envelope` (status,
token totals, context use) on every step. So tokens and state come from the
newest envelope in the journal's tail, falling back to the snapshot's.

**A journal line can be bigger than the tail that is read.** Each model call's
`action_intent` record embeds the entire prompt, so a segment runs to megabytes
and a 256 KB tail can start in the middle of one. Records are written with
sorted keys, which puts `type` last, so `record_type` reads it off the end of a
line that cannot be parsed. That is how a model call still in flight reads as
working even when the envelope before it is out of reach.

**A question leaves a record behind.** An approval prompt is a
`callback_registered`, answered by a `callback_resolved` with the same
`callback_id`. One with no answer after it is a held prompt, and the row says
`Asking`, which a transcript of any other harness here cannot tell apart from
working.

**The model is recorded only when it changes.** A session that never switched
model has no model anywhere in its store, so it falls back to `active_model` in
`config.toml`, the default a fresh session starts on. Discovery leaves the
unified layout's model empty rather than read the snapshot for every session on
every walk; extraction finds it, and the loader copies it onto the row.

**Tool counts come from the checkpoint, so compaction loses calls.** The
checkpoint is the model's context, and a compaction replaces the calls it
summarises. The projection's history is no better (it keeps a window, not the
session), and the journal reaches back only to the last snapshot. A tool
message whose `outcome` is `failure` is an error; the Python layout records
outcomes only as `stats.tool_calls_failed`, so its per-call detail never says
failed.

**The Python layout cannot say what was cached.** Its `stats` count prompt
tokens without the cached part, so its sessions price every input token at the
full rate and read high, where the unified store's `cachedInputTokens` is billed
at Mistral's cached rate.

**Every step creates files.** Chunks, journal segments and generations, each one
a create. The watcher would hold a re-walk open for each of them, so everything
inside a session directory except its `meta.json` is noise to it
(`is_vibe_noise`), as Gemini's `tool-outputs/` is.

## Columns

| Column | Source |
|---|---|
| `$` | estimated: the tokens at Mistral's published rates (`MISTRAL_TABLE` in `pricing.rs`, from docs.mistral.ai/inference/pricing), cached input at the cached rate, else LiteLLM's. The Python CLI's `session_cost` is ignored: it is priced from the user's own config, and the unified store records none |
| Tokens | the envelope's `tokenUsage` (input with its cached part inside it), or `stats` |
| `CTX%` | `contextUsage.totalTokens` (or `stats.context_tokens`) against LiteLLM's window for the model; none for a model LiteLLM does not know |
| Tools / `ERR%` | checkpoint `tool_call` parts and tool `outcome`s; `messages.jsonl` `tool_calls` and `stats.tool_calls_failed` |
| State | journal: unanswered callback → asking, a model call newer than every envelope → working, else the envelope's `status`: `idle` → waiting, `running` → working. Python layout: the last message |
| Process | `vibe-rs`, the frontend the `vibe` launcher execs into, matched by working directory or `--resume <id>` |
| Resume | `vibe --resume <id>` |

## What is not read

- **Permission mode.** Vibe's is the agent a session started with (`--agent
  auto-approve`, `--yolo`), which the store does not record.
- **Hooks.** `cctop hook` does not speak Vibe's hook dialect, so nothing is
  installed.
- **MCP servers.** `config.toml` lists them as an array of tables
  (`[[mcp_servers]]`), which the Codex-shaped TOML reader does not take.
- **Timestamps per message.** Neither layout stamps one, and usage is only ever
  a running session total, so a session's whole cost and tokens are filed under
  the moment it was last active. That is exact for a session inside one day,
  which is nearly all of them; one that spans midnight counts entirely on its
  last day, and resuming an old session moves its whole cost to "today" and
  "last 60 min". Tool details carry no time.
- **The Python-only CLI's process.** It runs as `python`, which `proc` does not
  descend into; its sessions are listed, but never as running.
- **Two sessions in one directory.** Both match the newest by working directory.
  `active/<id>.lock.json` names each session's app-server pid, which would
  attribute them exactly.
- **The turn in flight in the conversation view.** The reader uses the
  checkpoint, so the unified layout's conversation ends at the last finished
  turn.
