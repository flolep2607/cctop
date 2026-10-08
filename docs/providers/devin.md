# Devin

[← all harnesses](README.md) · parser: [`crates/core/src/session/devin.rs`](../../crates/core/src/session/devin.rs)

The only harness here that keeps its conversation in **two** places: a SQLite
database the CLI is working from, and a transcript it writes beside it for
other tools to read. Neither half is complete, so most of what is on this page
is about which of the two a given field comes out of — and about the one number
Devin records nowhere, which is the price.

## Where it lives

`~/.local/share/devin/cli/` — the platform data directory plus `devin/cli`
(`config::DEVIN_CLI_DIR`) — holding `sessions.db` and `transcripts/<id>.json`
beside it.

`$CHISEL_SESSION_DB` overrides it, and it is the one override that names a
**file** rather than a directory: cctop takes the parent of the database it
points at as the CLI directory, because the transcript has to sit beside the
database it belongs to (see `db_for`). Pointing it at a database somewhere else
therefore moves the transcripts with it, which is the only way that override can
be right.

Every other home's copy is found by convention, through `devin_cli_dirs()`. Only
this user's own database honours the variable; another user's is always
`~/…/devin/cli/sessions.db`, since an override cannot speak for a home it is not
in.

**The configuration is somewhere else entirely.** `~/.config/devin`
(`DEVIN_CONFIG_DIR`) holds `AGENTS.md`, `config.json`, `mcp_config.json` and
`skills`, none of which is next to the sessions. `access.rs` spent a while
looking in the data directory for a `config.toml` that has never existed there,
so the two are separate constants on purpose and the Access panel reads the
config one.

## The format

The database, opened read-only (`SQLITE_OPEN_READ_ONLY | NO_MUTEX`):

| Table | What cctop takes |
|---|---|
| `sessions` | `id`, `working_directory`, `model`, `created_at`, `last_activity_at`, `title`, `hidden`, and `agent_mode` for the permission column |
| `message_nodes` | `chat_message` (a JSON blob per node: `role`, `tool_calls`) and `metadata` (`num_tokens_preceding`), newest by `row_id` |
| `tool_call_state` | `tool_call_json` and `tool_call_update_json` per call: the arguments, and the `status` each update reported |

The transcript is one **ATIF-v1.7 JSON document** per session — not JSONL, and
not a database. `schema_version`, `session_id`, `agent` (with `model_name` and
the tool *definitions*, which are the toolset and not its usage), then `steps`:
each with `source` (`system`, `user` or `agent`), `timestamp`, `model_name`,
`metrics`, and `tool_calls`. Totals live at the end under `final_metrics`.

## The traps

**The two stores answer different questions.** The transcript is the artefact
other tools are meant to read; the database is the live state. Which is why the
tool *outcomes* are only in the second: a step records that a call was made,
and `tool_call_state` records how it ended. `tool_statuses` joins them on
`tool_call_id`, and a call with no row in it counts as **not** failed rather than
as unknown — a still-running call is not a failure, and guessing either way would
put a wrong number in `ERR%`.

**The transcript is one document, so nothing can tail it.** Every other JSONL
harness here answers "what is it doing" by reading the last 64 KB; a single
pretty-printed JSON file has no records to walk, and a seek into its middle
lands in the middle of a string. So `live_state`, `extract_last_tool` and
`extract_context` all read the database instead, and `session::live_state` hands
Devin over before the tail walk starts. The conversation reader reads the whole
document for the same reason, through its own `read_devin` rather than
`extract::for_each_jsonl`.

**One model, two spellings, in one file.** Most steps record the slug the API
was called with (`swe-2-high`); a few carry the display name the database knows
(`SWE-2 High`). Bucketing on the raw string splits one model into two rows in
the Cost panel, so `canonical_model` folds to letters and digits lowercased and
the *slug* is the spelling shown — it is what the pricing tables key on.

**`agent` steps only.** `system` and `user` steps carry no `metrics` and no tool
calls; counting from them would double the tokens against `final_metrics`.

**`created_at` is seconds, not milliseconds.** Every other harness stamps
milliseconds, so `ms_to_rfc3339(created_at * 1000)` is the whole conversion. Read
as-is, every Devin session is dated 1970.

**ATIF keeps no structured patch.** An `edit` or `write` diff lives in the call's
`arguments`, so the report's Changes view has to be built from
`extract::edit_delta` rather than read off the step.

## Cost

**There is none.** Neither the transcript nor the database records a dollar
figure, so `list_sessions` sets `cost_available = false` and `total_cost =
None`, and the `$`, `$/1H` and `$/24H` columns read `─`. Tokens are a different
question and are recorded: `Provider::records_token_usage` is true for Devin,
which is why `doctor`'s unpriced-model check skips it — a Devin `$0.00` is a
blank, not a price.

`--plan max` counts Devin as bundled (`Plan::includes`), alongside Claude. On
screen that changes nothing: `incl` stands for a figure cctop computed and then
withheld, and `cost_available = false` is tested before it, so a Devin row reads
`─` under every plan.

## Context

`message_nodes.metadata.num_tokens_preceding` on the newest node is what the
next request would carry in. The **ceiling is LiteLLM's `max_input_tokens` for
the model, or nothing** — the same qualification as OpenCode's, and a model
LiteLLM has never heard of shows a blank `CTX%`. `compacted` is always false:
ATIF records no compaction, so the CTX% chart has no markers.

## Tokens

`final_metrics` is the session's ledger — `total_prompt_tokens`,
`total_completion_tokens`, `total_cached_tokens`. The per-step `metrics` are
what fill the hourly and daily buckets, so a session whose steps carry no
timestamp contributes to the total and to no window, rather than to an invented
one.

**What the format does not settle:** whether `total_cached_tokens` is part of
`total_prompt_tokens` or additional to it. cctop keeps them in separate buckets
and prices them separately, on the reading that the cached part of a prompt is
billed cheaper rather than twice. A real transcript is the only thing that can
settle it.

## Liveness

`devin acp` is the agent backend, spawned per session by the CLI and by editor
integrations; `proc::could_be_agent` recognises the binary, and `provider_of`
requires the `acp` subcommand so that `devin auth`, `devin mcp` and the rest
never become a phantom session row. `devin --resume <id>` puts the id on the
command line, and `Session::resume_argv` is that command — so this is one of the
harnesses with a real process and real CPU and memory columns.

`agent_mode` on the session row answers the permission column, translated into
the four buckets: `normal` and `auto` are `ask`, `accept-edits` is `edits`,
`plan` is `plan`, and `dangerous`/`yolo`/`bypass`/`autonomous` are `BYPASS`.
`smart` — edits auto-approved plus a fast model running clearly-safe actions —
has no honest bucket, so it reports nothing rather than picking one.

cctop installs no hooks into Devin: it is not among `hook::HARNESSES`, and
Devin's own `hooks.v1.json` is a shape `cctop hook` does not speak. Everything
on a Devin row is forensic, read out of what the CLI left behind.

## What else it can reach

The conversation reader works (`serve/chat.rs::read_devin`), because ATIF keeps a
call and its result together on one step under `observation.results`, keyed by
`source_call_id`. Two Devin-specific details in it: `system` steps are mostly
prompt assembly — the `sysprompt` and `rules` telemetry sources, plus
`<available_skills>` and `<system_info>` blocks re-injected each turn — and only
a *tagged* block reporting an event is shown as a turn; and `step_id` is a
number, not a string, so two consecutive steps stay two replies.

`delete` is the one destructive path that writes to the harness's own store. It
sets `hidden = 1` and removes the transcript, rather than deleting the row: the
live CLI holds the database open, and hiding is what the CLI itself does.