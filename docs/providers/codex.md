# Codex

[← all harnesses](README.md) · parser: [`src/session/codex.rs`](../../src/session/codex.rs)

Codex writes a *rollout*: an append-only log of the events the agent went
through, rather than a conversation. Most of the parser's weight is not in
reading it but in one field — `exec`, which arrives as JavaScript source.

## Where it lives

`$CODEX_HOME/sessions/`, defaulting to `~/.codex` (`config::CODEX_HOME`).
Codex files rollouts under `YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`, but
discovery uses `config::rglob(root, ".jsonl")` — any depth, any name — so a
layout change costs nothing.

Files are sorted by *filename*, not path, before anything is read: the rollout
name carries its timestamp, and sorting by path would order by whose home the
file sits in first.

Codex is the second harness with profiles: `~/.codex*` directories holding an
`auth.json` are accounts, selected by `$CODEX_HOME`
(`config::PROFILED`). A subscription runs out of window before the day does,
which is the reason both harnesses grew the concept.

## The format

JSONL. The records cctop reads:

- `session_meta` — `payload.id`, `payload.cwd`, `payload.timestamp`.
- `turn_context` — `payload.model` and `payload.effort` (or
  `reasoning_effort`). The model can change mid-session, so the last one wins.
- `event_msg` with `payload.type == "token_count"` — the usage figures, under
  `payload.info.last_token_usage`.
- `function_call` / `custom_tool_call` and their `*_output` counterparts,
  either at the top level or wrapped in a `response_item`.
- `web_search_call`.

The session id is the trailing UUID of the filename
(`config::trailing_uuid`), falling back to the first `session_meta`. That
ordering matters: **a forked rollout carries a second `session_meta` naming the
session it was forked from**, and letting a later one win would give the file
another session's identity.

## Traps

**`exec` is JavaScript, not arguments.** Codex hands the wrapper a source
string — `await tools.exec_command({cmd:"ls"})` — so there is no JSON to read.
`unwrap_exec_all` lifts every `tools.*` call out of it, and `quote_bare_keys`
turns the object literal into JSON; without that, roughly two thirds of Codex
tool calls reached the activity pane with no arguments at all, losing the
command or the file that makes the line worth reading. `bound_json_string`
resolves the common `const patch = "…"; tools.apply_patch(patch)` shape, and
`find_outside_strings` skips `tools.` occurrences inside a quoted patch.

One entry can delegate to several tools — the agent batches them as
`Promise.all([...])` — and each is a real call, so all of them are recorded.
Batched calls share the entry's `call_id`, and therefore its measured duration:
they ran concurrently, so a common window is the honest reading.

*ponytail:* the count is of calls written in the source. A runtime fan-out such
as `ids.map(id => tools.write_stdin({session_id: id}))` is one call textually
and many at execution, and still counts once.

**Tool names are renamed to match everything else.** `normalise_exec_tool` maps
`exec_command` → `Bash` (with `cmd` → `command`), and `view_image` /
`read_mcp_resource` → `Read` (with `path` / `uri` → `file_path`). This is what
lets one activity renderer serve every harness.

**There is no `is_error`.** `output_failed` reads the sandbox's own summary
line — `Script failed`, `Script error:` — and, for the structured form, the
JSON-inside-a-string `metadata.exit_code`. Anything unrecognised counts as
success, so an unfamiliar output shape leaves a call unmarked rather than
falsely flagged.

**`input` already contains the cached part.** `last_token_usage.input_tokens`
is the total; only the uncached remainder bills at the full input rate, which is
why `extract` computes `input = input_total - cached_input` before pricing.

**A rollout with no `token_count` yet is not an error.** It returns tools and a
model with no cost, rather than a `SessionData::error` — unlike Claude, where
billable usage under an unknown model is a hard failure.

**A multi-file `apply_patch` only contributes its first file** to the recent-
writes list, because the summary is `first.rs (+3 more)` and the rest are not
recoverable from it. That is a documented limit in
`session::distil_recent_writes`, and it means the collision warning can miss a
file a Codex session is holding.

## Cost

Estimated. `pricing::resolve_codex` — a built-in table, LiteLLM behind it — and
the resolved per-million rates are carried on `SessionData::rates` so the Cost
panel can show what it charged rather than only the total. Policy in
[What the cost figures mean](../costs.md).

## Context

From the same `token_count` event: `info.last_token_usage.input_tokens` for
what is used, `info.model_context_window` for the ceiling, falling back to
`config::CODEX_DEFAULT_CTX` (258,400). Codex is the only harness that states its
own window size, which is why nothing here has to consult a table.

No compaction accounting: the rollout does not say when the window was
reclaimed, so `compacted` is always false and the CTX% chart has no markers.

## Liveness

The hardest of the seven, and the only one where cctop deliberately refuses to
guess. `codex resume <uuid>` matches directly. But the app-server — which is
what actually runs most sessions — carries no rollout UUID on its command line,
so an unmatched Codex PID falls through to the working-directory match, walking
*ancestors* to find one: the worker is often a child of `codex-linux-sandbox`
and has no useful cwd, while the parent carries the managed workspace in
`--command-cwd` or `--sandbox-policy-cwd`.

If that still finds nothing, the PID is dropped rather than turned into a row.
Every other provider gets a synthetic "running, no transcript" row for an
unmatched process; Codex does not, because a manufactured rollout-less session
is worse than a missing one (`proc.rs`, in the fallback loop).

Codex is also the harness with two hook surfaces:
`$CODEX_HOME/hooks.json` says everything, and the `notify` program in
`config.toml` says only that a turn ended but works the moment it is written.
`--install-hooks` writes both for the user scope.
