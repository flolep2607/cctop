# What the cost figures mean

[← back to the README](../README.md)

Claude, Codex, and Gemini costs are **estimates**: tokens multiplied by published
per-token rates, taken from built-in tables and falling back to the
[LiteLLM](https://github.com/BerriAI/litellm) database (cached for 24 hours).
OpenCode and Pi already persist provider-calculated costs, which cctop reads
directly.

Gemini records a per-turn token breakdown but no cost. Its `cached` count is the
part of the prompt served from context cache rather than an addition to it, so
only the uncached remainder is priced at the full input rate; thinking tokens are
priced as output, which is how Google bills them. A Gemini session on a bundled
Code Assist tier will still show an estimate — the transcript carries nothing
that says which tier it ran on, and the figure is honest about what the tokens
would cost at retail.

Cursor native-agent transcripts expose projects, conversation activity, and
tool calls, but not model names, tokens, context usage, costs, or a dedicated
per-session process. Those fields display as unavailable; live status means the
transcript has changed within the last 90 seconds.

Windsurf goes further: its workspace state records the conversation and its tool
calls but no model, tokens, or cost at all — the credit accounting lives on
Codeium's servers. Those sessions report cost as unavailable rather than as free,
and their timestamps come from the workspace database's mtime, which is the only
clock Windsurf leaves behind.

Subscription plans — Claude Max, Pro, Team — are flat-rate or bundle tokens
differently, so these numbers will not match your invoice. Treat the `$` column
as a measure of resource consumption, not as billing. Use `--plan max` or
`--plan included` to display bundled usage as `incl` instead.

## Where each provider's data comes from

| Source | Path |
|--------|------|
| Claude Code (CLI) | `~/.claude/projects/<slug>/<uuid>.jsonl` |
| Claude for Mac | `~/Library/Application Support/Claude/{claude-code,local-agent-mode}-sessions/` |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| Cursor | `~/.cursor/projects/*/agent-transcripts/*/*.jsonl` |
| Gemini CLI | `~/.gemini/tmp/<project>/chats/session-*.json{,l}` |
| OpenCode | `~/.local/share/opencode/opencode*.db` (platform data directory) |
| Pi | `~/.pi/agent/sessions/**/*.jsonl` |
| Windsurf | `<Windsurf User dir>/workspaceStorage/*/state.vscdb` |

`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `CURSOR_HOME`, `GEMINI_DIR`,
`OPENCODE_DATA_DIR`, `PI_CODING_AGENT_DIR`, `PI_CODING_AGENT_SESSION_DIR`, and
`WINDSURF_USER_DIR` are honoured. Caches live in `~/.cache/cctop/`.

Gemini and Windsurf sessions are read from disk but not matched to a running
process: neither takes a session id on its command line, so there is nothing to
tie a PID to a transcript. Their rows always read as stopped, and the CPU and
memory columns stay blank rather than showing another session's figures.

The left status dot is green while an agent is working, amber after its latest
response is waiting for your input, and red when the newest transcript event is
an API error. A hollow grey dot is a stopped session, and a filled `◉` is the
session that rang in the last 30 seconds.
