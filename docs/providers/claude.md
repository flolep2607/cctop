# Claude Code

[← all harnesses](README.md) · parser: [`src/session/claude.rs`](../../src/session/claude.rs)

The harness cctop reads most deeply, and the only one whose transcript answers
every column. Everything else on this page is a consequence of that: the
context breakdown, the compaction chart and the subagent list exist because
Claude Code writes enough down for them, not because they were designed
provider-neutral and only implemented once.

## Where it lives

`$CLAUDE_CONFIG_DIR/projects/<project-slug>/<uuid>.jsonl`, defaulting to
`~/.claude` (`config::CLAUDE_CONFIG_DIR`). A file is a session only if its stem
is a full UUID — `config::is_full_uuid` in `list_sessions` — so a stray
`.jsonl` in a project directory is ignored rather than half-parsed.

The project slug is not read for anything. The working directory a row shows
comes from the `cwd` field inside the transcript (`collect_static`), because
the slug is a lossy encoding of a path and the transcript states it outright.

Three things widen the search beyond that one directory:

- **Profiles.** `$CLAUDE_CONFIG_DIR` lets one machine hold several logins, each
  with its own `projects/`. cctop reads *all* of them: any `~/.claude*`
  directory holding a `.credentials.json` counts as an account
  (`config::PROFILED`), and `config::profile_for` stamps the PROFILE column
  from the transcript's path. Reading only the directory the env var named was
  a real bug — a running session showed as a row with a process and no model,
  because its transcript was in a profile cctop was not looking at.
- **Other homes.** `$CCTOP_ALL_USERS` and `$CCTOP_HOMES` add other users' homes
  to every root; `Session::owner` is stamped from the path in `list_all`.
- **Claude for Mac.** `~/Library/Application Support/Claude/`
  `{claude-code,local-agent-mode}-sessions/<account>/<device>/local_*.json` is
  metadata, not a transcript: `cliSessionId` points at a real Claude Code
  transcript nested under the session directory's own `.claude/projects`, which
  `find_desktop_jsonl` locates. An entry flagged `isArchived` is skipped, and a
  `vmProcessName` marks the Cowork surface (the agent runs in a cloud VM, so
  there is no local process — see the liveness section). The label comes from
  `userSelectedFolders[0]`, falling back to `cwd`.

Subagents get their own files: `<transcript-stem>/subagents/<agent-id>.jsonl`,
with an optional `<agent-id>.meta.json` beside them. `transcript_files` in
`session/mod.rs` is what makes a session mean "the main file plus its
sidechains" everywhere it matters — including the mtime, since a subagent
streams into its own file without touching the parent's.

## The format

JSONL, one record per line, `type` naming the kind: `user`, `assistant`,
`system`, `attachment`, `custom-title`, `ai-title`. Discovery reads only the
first 50 lines (`collect_static`) for the model, cwd, start time and launch id,
and the last 200 (`scan_custom_title`) for a `/rename`, because a transcript can
be hundreds of megabytes and the table needs neither.

A session whose first 50 lines name no model is dropped from the table
entirely. That is an agent that started and never reached the API, and it has
nothing to show.

Billable usage rides on `assistant` records as `message.usage`:
`input_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`, and a
`cache_creation` object splitting the last into `ephemeral_5m_input_tokens` and
`ephemeral_1h_input_tokens`. Older transcripts carry only the aggregate, so the
remainder after the 1h tier is attributed to 5m, which is the default TTL.

## Traps

**Streaming repeats a request.** The same `requestId` is written many times with
growing counts. `visit_assistant` keeps only the last snapshot per key
(`last_by_key`), and the totals are flushed once the file is read. Summing as
you go inflates every figure in the session.

**Parallel tool calls are not one record.** The API shape suggests one assistant
message with several `tool_use` blocks; Claude Code writes *one record per
block*, all sharing a `requestId`. So counting blocks inside a record answers
"1" for every call in a fan-out. `attach_call_details` counts across the whole
file instead, and stores the answer as `ToolDetail::shared` — anything that
divides a turn's tokens by that count, or attributes the turn's window growth to
a single call, is wrong for every fan-out without it.

**A resume forks.** `claude --resume X` does not reopen X. It writes a new file
under a new id which records X as the id it was *launched* with — the
`session_id` field, as against `sessionId`, which is the file's own. That
launch id is what `Session::launch_id` holds, and matching a running process on
it is what stops the live agent being attributed to the finished conversation.
See `proc::resumed_key`, whose doc comment is the long version.

**An oversized line used to lose its cost.** A base64 image or a whole-file
`Write` can push a line past `MAX_JSONL_LINE_BYTES`, and parsing it into a
`Value` is what blows up memory. `extract::slim_oversized` re-reads such a line
into a small object carrying only `usage`, so the turn's tokens survive; a huge
*user* entry, which carries no usage, is still dropped.

**Sidechain turns are billed but not in the window.** Older transcripts
interleave subagent turns into the parent file flagged `isSidechain`. They cost
this session real money, so they count towards cost — but they ran against a
different context window, so `in_context` keeps them out of the breakdown, and
`last_main_model` keeps a subagent's model from naming the session.

**An unknown model is an error, not a zero.** Billable usage under a model
`resolve_claude` cannot price sets `SessionData::error`, which the cache refuses
to persist. A fabricated `$0.00` would look exactly like a free plan.

## Cost

Estimated: tokens times published rates, `pricing::resolve_claude` with a
LiteLLM fallback. The general policy, and why the number will not match your
invoice, is in [What the cost figures mean](../costs.md).

## Context

The only harness with a full breakdown, and the only one where a compaction is
visible at all.

`total` and `startup` are measured — they are the window figures the API itself
reported, `startup` being the first request of a segment, whose input is the
system prompt, tool schemas, CLAUDE.md and skills index with no conversation in
front of it. Everything else is estimated from transcript characters at
`CHARS_PER_TOKEN = 2.75`, a constant fitted across 167 local sessions rather
than assumed; it is low against the prose rule of thumb because this content is
code, JSON and paths. Whatever the parts do not account for is `unaccounted`,
reported signed, because "the categories add up to more than the window" is
information rather than an error.

A *segment* is one stretch from a start or a compaction to the last request
that measured itself. Mixing segments is the one way the type can lie without
any number being wrong, which is why `note_compaction` retires the whole
segment at once and seals it: a session that compacted and then stopped has no
request in its new segment, so the sealed one is reported whole and flagged
`superseded`.

Compactions are recognised two ways — `isCompactSummary` on the summary entry
in newer transcripts, `system`/`compact_boundary` in older ones — and both the
full extractor and the tail scan look for both, or the column and the panel
would disagree about the same file.

The window's *size* is resolved in one place, `resolve_ctx_max`, so nothing can
name two ceilings for one window: a `[1m]` model string, then a pinned `model`
in `.claude/settings.local.json`, `.claude/settings.json`, or the config root's
`settings.json`, then LiteLLM, then 200k. Observed usage outranks all of them —
a settings file naming a 200k model must not report a 1M session as 239% full.

Assistant `thinking` blocks are deliberately not counted: Claude Code writes
them with an empty `thinking` string and only the signature survives, so there
is nothing to measure and it lands in the unaccounted gap honestly.

## Subagents

Built by `build_subagents` from the sidechain files plus the parent's `Agent`
`tool_use` records. Three cases, all of which have bitten:

- A transcript still being appended to outranks the parent's `tool_result`. A
  background subagent is acknowledged the moment it *starts*, so taking that
  result at face value marked every one of them finished three seconds in.
  Silence for `SUBAGENT_QUIET_MS` (30s) is what counts as done.
- Older agents wrote no `toolUseId`, so they are paired back to a parent
  `tool_use` by description, nearest start time winning.
- Claude Code purges old subagent transcripts but keeps the parent's
  call/result pair, so a row is synthesised from what survives and marked
  `ghost`.

## Liveness

A per-session process, matched by `--resume <uuid>` forwarded through the fork,
or by working directory when nothing on the command line says. Claude for Mac
resumes by window *title* rather than id, so that is matched separately. Cowork
sessions run in a VM and have no local process at all: activity within 90
seconds stands in, and CPU and memory stay blank by design.

The binary is not always called `claude` — the native installer and the web
harness ship it as a version-named executable — so `is_claude_binary` trusts
`argv[0]` and the install path as well as the process name.

`permissionMode` is stamped on every `user` record, so the permission column
works without hooks; the newest one wins, since the user can change it
mid-session. What the transcript cannot express is an agent *blocked* on a
question: a held permission prompt writes nothing, so the newest record is a
tool call in flight, which reads exactly like work. Only the hooks
`--install-hooks` writes into `$CLAUDE_CONFIG_DIR/settings.json` say that
outright.
