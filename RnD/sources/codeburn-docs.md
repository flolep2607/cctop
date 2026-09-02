# codeburn documents, digested

Read **2026-09-02** from `getagentseal/codeburn` at `main`. These are condensed
notes taken while reading, kept so a claim in
[../codeburn.md](../codeburn.md) can be checked without fetching again — and so
that when the upstream docs change, the version we reasoned about is still here.

Documents read:

- `README.md`
- `docs/architecture.md`
- `docs/optimize.md`
- `docs/design/codeburn-mcp.md`
- `docs/design/codeburn-mcp-plan.md`
- `docs/design/perf-cache-fix.md`
- `docs/providers/` (index only)
- `docs/sync/DEVELOPER.md`

---

## README

Free, MIT, local-only spend tracker across 41 AI coding tools. Reads session
files already on disk; no wrapper, no proxy, no API keys. Node 22.13+ (22.15+
for Zed, which needs zstd). Installs via npx / npm -g / bunx / pnpm dlx / brew.

Pitch: "The bill tells you the total. It never tells you that half of it went to
conversation instead of code."

**Surfaces.** An Ink TUI (period keys `1`–`6`, `c` compare, `o` optimize, `p`
provider toggle, `b` back, `j`/`k` by day, refreshes at most once a minute); a
localhost web dashboard on :4747 with 15-minute / hourly / daily granularity;
menu-bar apps for macOS (Swift), Windows (Tauri tray, .msi) and GNOME (shell
extension).

**Commands.** `report` (default), `today`, `month`, `overview`, `optimize`,
`compare`, `yield`, `doctor`, `audit`, `context`, `guard`, `mcp`, `models`,
`model-alias`, `price-override`, `model-savings`, `model-flat-rate`,
`proxy-path`, `plan`, `currency`, `sync`, `share`, `devices`, `export`,
`status`, `act`. Common flags: `--provider`, `--project`, `--exclude`,
`--from`/`--to`, `-p today|week|30days|month|all|lifetime`, `--format json`.

**Four token types** normalised everywhere: input (fresh), output, cache read,
cache write. Claude fast mode applies a multiplier. Cursor output is estimated
from reply text and its cache is server-side only, so marked estimated.

**Pricing.** LiteLLM daily, cached 24h in `~/.cache/codeburn/`. Hardcoded
fallbacks for all Claude and GPT-5 models. Routing gateways peeled to upstream
model. Overrides in USD per 1M tokens.

**Plans.** Per provider and coexisting; presets priced April 2026; Copilot in AI
credits (Pro 1,500 / Pro+ 7,000 / Max 20,000). One overage row per active plan.

**Currency.** 162, from Frankfurter (ECB), cached 24h, applied to dashboard,
status bar, menu bar and exports.

**Task categories** (13, deterministic, no LLM calls): Coding, Debugging,
Feature Dev, Refactoring, Testing, Exploration, Planning, Delegation, Git Ops,
Build/Deploy, Brainstorming, Conversation, General.

**One-shot rate.** File-aware: `Edit foo.ts → Bash → Edit foo.ts` is a retry;
editing a different file is not. File-level tracking for Claude, Codex and
Goose; others fall back to tool-name detection.

**Optimize.** Classes fix / nudge (habits) / keep (FYI). Findings carry
estimated token and dollar savings labelled measured or estimated, a
copy-pasteable remediation, an A–F health grade, and a new/improving/resolved
status over a 48-hour window. `--apply`, `--apply --yes`, `--apply --dry-run`;
`act list`, `act undo <id>` (refuses if files changed since apply), `act report`
(estimated vs realised after 3+ days).

**Guard.** Hooks into `~/.claude/settings.json`. Soft cap $5 (one warning), hard
cap $15 (stops session, `guard allow` lifts), checkpoint $3 (nudge if session
ends with no edits or commits), session openers for wasteful projects, optional
status-line cost. Config `~/.config/codeburn/guard.json`, `null` disables. Hooks
fail open.

**Compare.** One-shot rate, retry rate, self-correction turns, cost per call,
cost per edit, output tokens per call, cache hit rate; plus per-category
one-shot, delegation rate, planning rate, tools per turn, fast-mode usage.

**Yield.** Git-commit correlation by timestamp window. Productive / reverted /
abandoned / ambiguous. Each commit to at most one session, tightest containing
window. Methodology printed as `timestamp-window`. Needs a git repo.

**Devices.** `share --pair` PIN, `devices add`, combined totals. Local network
only.

**Doctor.** Per provider: paths probed with env overrides, session count, parse
health, cached files, verdict OK / NOTHING FOUND / ERRORS. `--json`.

**Audit.** Per-provider-per-model token *source* table.

**Multiple Claude config dirs** via `CLAUDE_CONFIG_DIRS`, `:`-delimited on
POSIX and `;` on Windows.

**Interpretation table** (their wording, condensed): cache hit < 80% → unstable
system prompt or caching off; many Reads → re-reading, missing context; coding
one-shot ~30% → retry loops; big model on small turns → overpowered;
`dispatch_agent`/`task` heavy → fan-out, expected or excessive; no MCP usage →
unused or broken config; Bash all `git status`/`ls` → exploring not executing;
Conversation dominant → talking not coding. Caveat: one session at 60% cache hit
is fine, 60% for weeks is a config problem.

**Privacy.** All local. Web dashboard binds loopback. Devices are LAN + PIN.
Vercel AI Gateway is the only cloud source, optional. MCP pseudonymises project
names.

---

## docs/architecture.md

Node CLI, `src/cli.ts`. Three native GUI clients (macOS Swift, Windows
Tauri+React, GNOME JS) that spawn the CLI as a subprocess and parse its JSON.
The CLI owns all business logic; clients are read-only presentation.

**Providers** (`src/providers/`) implement a common interface: discovery,
session parsing, model and tool naming. Thirty load eagerly; eleven (Cursor,
SQLite-backed, network clients) are dynamic imports so users without those tools
do not pay for the dependency.

**Pipeline:** provider discovery → per-file and per-line parsing →
deduplication against a shared `seenKeys` Set → ProjectSummary aggregation →
daily cache layer → output formatting. `src/parser.ts` is the aggregator and
threads the dedup set into every provider parser, so a turn appearing in two
providers (Claude's log and Cursor's mirror) counts once.

**Parallel cold parse** (`src/parse-workers.ts`). Per-file work goes to
`worker_threads`. Each worker runs the same parse function against an *empty*
dedup set and returns results as JSON plus the keys it claimed. The parent
installs results **in order** and owns all cross-file state (dedup, project
canonicalisation, Codex result caching). Replayed keys collide, which triggers
in-process re-parsing — that is what makes forked Codex rollouts safe.

Gates: serial if pending bytes < 200 MB, or cores ≤ 2, or available memory
< 4 GB. Otherwise workers = `min(cores - 1, thread_budget / per_worker,
pending_files / 50 OR pending_bytes / 200MB)`. Per-worker memory budget
`clamp(256MB, 2 × (pendingBytes / pendingFiles) + 128MB, 1GB)`.
`CODEBURN_PARSE_WORKERS=N` overrides all gates; `CODEBURN_VERBOSE=1` logs the
decision.

**Caches**, all under `~/.cache/codeburn/`, all atomic temp+rename at 0o600 with
a numeric `version` field that forces recompute when bumped:

| File | Owner | Invalidation |
|---|---|---|
| `codex-results.v<n>.json` | `src/codex-cache.ts` | mtime + size per `.jsonl` |
| `cursor-results.v<n>.json` | `src/cursor-cache.ts` | mtime + size of the SQLite db |
| `daily-cache.json` | `src/daily-cache.ts` | tracks computed date, backfills new, reuses old |

`src/session-cache.ts` stores per-provider-per-month shards; a date-range query
reads only the relevant shards unless `CODEBURN_CACHE_SCOPE=all`.

**Optimize** is `src/optimize.ts`: 14 detectors ranked by impact, each returning
`WasteFinding | null` carrying `WasteAction` objects (paste suggestions, edit
prompts). Detectors named: junk reads (node_modules, .git), duplicate file
reads, underutilised MCP servers, bloated CLAUDE.md, low edit ratio, cache
bloat, ghost agents, ghost skills, ghost commands, oversized shell limits,
low-value sessions, context ratio anomalies.

**Native clients.** macOS: `NSStatusItem` popover, spawns via `/usr/bin/env`
never a shell, decodes `MenubarPayload` mirrored from `src/menubar-json.ts`,
validates argv against an allowlist regex, augments PATH for Homebrew and npm.
Windows: Tauri 2, Rust owns tray and spawning, `cli.rs` searches only absolute
PATH directories, allowlists `CODEBURN_BIN`, gates startup on `MIN_CLI_VERSION`,
spawns system tools by absolute path to avoid current-directory hijack. GNOME:
plain JS, no bundler, `Gio.Subprocess`, caches results 300s.

**Build.** `npm run build` bundles the LiteLLM pricing JSON into
`src/data/litellm-snapshot.json`, then tsup emits an ESM bundle to `dist/cli.js`.
Reproducible because the pricing snapshot is checked in.

**Tests.** 192 vitest files across `tests/`, `tests/providers/`,
`tests/security/`, `tests/sharing/`. Three cache-lock suites run serially
(`npm run test:locks`) because cross-process file locks contend. CI runs Semgrep
on hot parsing paths plus the full suite on every PR and push to main.

**Their own list of distinctive decisions:** single-threaded state model;
adaptive parallelism; the CLI-JSON output contract as the client boundary; lazy
provider loading; allowlist validation on every spawn from a native app.

---

## docs/optimize.md

Thinner than the README on this subject. Confirms: scans transcripts for the
period (tool calls, per-call token usage, turn retries) plus configuration;
classes `fix` / `nudge` / `keep`; measured (provider-counted) vs estimated
(model-derived); changes are logged and revertible; an applied fix is a claim
verified against subsequent usage.

Detectable waste listed: repeated file reads across sessions, low read-to-edit
ratio, unused MCP servers, ghost skills and agents, oversized config files,
context-heavy sessions, Bash output overflow.

---

## docs/design/codeburn-mcp.md and codeburn-mcp-plan.md

**Shape.** Resident in-process stdio server, *not* exec-per-call — they measured
a **17.6 s** warm-start penalty per call otherwise, because a fresh process
cannot inherit the 180-second session cache. Pre-warms the `today` dataset at
boot. In-flight coalescing: concurrent identical requests (same period, same
optimize flag) share one scan.

New modules: `usage-aggregator.ts`, `mcp/server.ts`, `mcp/tables.ts`,
`mcp/redact.ts`. Two new deps (MCP SDK, Zod), both externalised from the bundle.

**Two tools**, both returning markdown tables *plus* typed JSON, both annotated
`readOnlyHint: true`, `idempotentHint: true`, `openWorldHint: false`:

- `get_usage` — period (default today), optional breakdown dimension
  (project / model / task / provider), limit (default cap 20 rows),
  `include_project_names`. Returns cost, calls, sessions, cache-hit %, one-shot
  rate, plus top-N tables. Fast (~3–5 s): skips the optimize scan.
- `get_savings` — period (default last 7 days), `include_project_names`.
  Returns optimize findings, retry tax, routing waste. Slower (+~13 s) because
  it runs `scanAndDetect`.

**Privacy.** Project and session names hashed to `project-<6hex>` via stable
SHA-256 by default; absolute paths never exposed; real names only when the
caller opts in per call. Rationale given: agent interactions get logged and
audited, and client or repo names should not leak into that.

**Refactor.** `status --format menubar-json`'s aggregation is extracted into
`buildMenubarPayloadForRange(periodInfo, opts)` so CLI and MCP share it, with a
parity test guarding the extraction. An `optimize` flag gates the expensive
`scanAndDetect`.

**Phasing**, eight tasks: deps and externalisation → move `buildPeriodData` →
extract the payload builder → redaction module → markdown renderers → server
with coalescing → wire the `mcp` command with a stdout JSON-RPC guard → full
suite and parity verification.

**Deviation they admit:** per-provider graceful degradation (a `degraded[]`
array) deferred to v2 to avoid destabilising the shared parser loop; v1 surfaces
a provider failure at the tool boundary as `isError: true`.

**Success criteria worth noting:** empty-data states must return a friendly
message, not a table of zeroes.

---

## docs/design/perf-cache-fix.md

**Problem.** `status --format menubar-json` took 25–90+ s per call with *no*
speedup on repeat calls against unchanged data. Two causes: re-parsing a
monolithic ~386 MB `session-cache.json` on every invocation (~20% of CPU), and
running the full aggregation over the whole period regardless of what changed.
Root cause: the menubar app spawns a fresh CLI per poll, so every process-local
TTL cache never accumulates value.

**Fix.** A disk-persisted snapshot keyed on two things.

1. **Corpus fingerprint** — a stat-only pass (`dev`, `ino`, `mtimeMs`,
   `sizeBytes`) across discovered sources. For Claude sources it expands project
   directories to actual `.jsonl` files first. It **deliberately does not
   fingerprint `session-cache.json` itself**, because that file only rewrites
   after parsing completes.
2. **Query key** — period/day/days, provider, project and exclude filters,
   optimize and timeline flags, Claude config. One snapshot file per distinct
   query, so scopes cannot collide.

On fingerprint+query match, serve the persisted payload with stat calls only.

**Settle window.** On a fingerprint mismatch, if
`Date.now() - newestMtimeMs < settleWindow`, serve the prior snapshot and never
persist during the defer. Once the window expires (default 2000 ms,
`CODEBURN_STATUS_SNAPSHOT_SETTLE_MS`) the next call recomputes and persists.
This coalesces rapid successive writes — a streaming assistant turn — into one
recompute without permanent staleness. They note the logic lives *entirely* in
the snapshot layer, not in `reconcileFile`, because pushing it down caused
correctness bugs from incomplete defer signals.

**Storage.** `status-snapshot.<queryKeyHash>.json` in the cache dir. Per-file
atomic temp+rename with a CAS re-read guard. A failed read or write falls back
to a full recompute. `--scope combined` device enrichment stays live and
uncached per call, so stale enrichment cannot leak between query types.

**Surgical surface.** `parser.ts` gains `computeCorpusFingerprint()` reusing
existing `discoverAllSessions`, `fingerprintFile`, `collectJsonlFiles`;
`session-cache.ts` gains `loadStatusSnapshot()`, `saveStatusSnapshot()`,
`statusSnapshotSettleMs()`; `main.ts` builds the query key and checks the
snapshot only in the `status --format menubar-json` branch. `reconcileFile` and
the aggregation internals are untouched, and the resident `serve` process keeps
its in-memory caching.

---

## docs/providers/ (index only)

`README.md` and `NEW_PROVIDER.md`, then one file per provider stating its exact
data location, storage format and known quirks. Split in the index between
eager and lazy loading.

Eager: claude, cline, cline-cli, codewhale, codex, copilot, devin, droid, dsh,
gemini, grok, hermes, ibm-bob, kilo-code, kimi, kimicode, kiro, lingtai-tui,
mistral-vibe, omp, openclaw, openclaude, pi, qwen, roo-code, zerostack.

Lazy: antigravity, crush, cursor, cursor-agent, forge, goose, opencode, warp,
vercel-gateway, zcode.

Shared helper: `vscode-cline-parser.md`.

---

## docs/sync/DEVELOPER.md

Uploads AI call telemetry to a backend as **OTLP/HTTP JSON**. Preview feature;
protocol may change.

**Discovery.** `GET {baseUrl}/.well-known/codeburn-export.json` returns the
issuer URL, OAuth client ID, scopes and the traces path (default `/v1/traces`).

**Ingest.** `POST {baseUrl}{traces_path}` with a Bearer token. The backend
derives developer identity from the JWT `sub` claim only — no PII in the
payload.

**Auth.** OAuth authorization-code with PKCE. Local callback server on fixed
ports 19876 / 19877 / 19878. Tokens refresh automatically on each push.
Credentials in Keychain / libsecret / DPAPI with a filesystem fallback.

**Payload.** Per-call spans: provider, model, input and output tokens, estimated
cost. Optional: project, tools used, speed, cache metrics, work-unit lineage,
subscription coverage, session duration. Their rule: "every attribute from
`ai.work_unit_id` down is optional and sent only when its value is proven", so
old receivers ignore unknown fields. Never code, never prompts.

**Dedup.** A local sent-ledger at `~/.cache/codeburn/sync-ledger.json` lists
successfully uploaded calls; entries older than six months are pruned. Their
justification: "Timestamp watermarks silently skip late-arriving calls… The
ledger is exact." Span IDs derive deterministically from the dedup key, so a
re-send is an idempotent upsert server-side.

**Commands.** `sync setup <url>`, `sync push`, `sync push --since 30d`,
`sync status`, `sync logout`, `sync reset --confirm` (clears the ledger,
re-sends everything next push).

No team or multi-user features are documented.

---

## THIRD_PARTY_NOTICES.md

Structure: their own MIT licence at the top, then one section per third-party
item giving the *context of use*, the full licence text where the terms require
it, and a trademark disclaimer for branded assets. Conceptual inspirations are
attributed alongside actual dependencies.

Three entries:

1. `@deepseek-ai/dsh-session-persistence-jsonl` — BSD 3-Clause. Used for
   `scanZstdFrames`, reading DeepSeek Harness session logs. They note upstream
   declares conflicting licences (BSD 3-Clause on npm, MIT in the monorepo
   source) and reproduce **the stricter of the two**.
2. Provider icons — MIT plus CC0 1.0, pinned to a vector-collection revision
   (`714bff0…`); JetBrains and Notion marks from Simple Icons under CC0. With
   the disclaimer that provider names and logos remain their owners' property.
3. **CodexBar** — MIT, credited as *prior art*, not a dependency: the
   inspiration for their provider usage tracking. Worth researching separately.
