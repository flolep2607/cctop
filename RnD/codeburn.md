# Ideas from codeburn

**Source:** <https://github.com/getagentseal/codeburn> — a Node CLI that reads
the same session files cctop does and reports what they cost. MIT, ~41
harnesses, three native menu-bar clients.
**Read:** 2026-09-02. Documents digested in [sources/codeburn-docs.md](sources/codeburn-docs.md).

Overlapping scope, different centre of gravity. codeburn is a *spend analyst*:
it looks backwards over months and tells you where the money went. cctop is a
*monitor*: it looks at what is happening now and which session needs you. The
useful consequence is that almost nothing here is a duplicate — codeburn has
built the retrospective half of a problem cctop only touches in passing, and
cctop has the live half codeburn does not attempt at all.

Nothing below was a commitment when it was written. Since then it has been read
and ruled on — see the verdicts.

---

## Verdicts

Decided 2026-09-02. The ranking at the end of this file is the *opinion* that
preceded these; where the two disagree, this table is what happened.

| | Idea | Verdict |
|---|---|---|
| #1 | `cctop optimize` — waste scanner | **Wanted.** Design in [optimize-and-compare.md](optimize-and-compare.md) |
| #4 | `cctop compare` — models on your own work | **Wanted.** Same design note |
| #6 | `guard` — budget caps as hooks | **Rejected.** Not what cctop is for. It also wanted the one thing `src/hook.rs` is documented never to do |
| #12 | Menu bar and tray clients | **Rejected.** cctop is used over ssh and inside WSL, where a tray is either invisible or on the wrong machine. The stable JSON contract underneath it survives on its own merits |
| #13 | Their MCP surface | **Rejected.** cctop's four tools are already better. The pseudonymised project names are the only part worth revisiting, and not now |
| #15–#18 | Corpus fingerprint, settle window, snapshot cache | **Built** in-process: `src/fingerprint.rs`, warm walk 90–158 ms → 12–14 ms. No persisted snapshot — see the note under #15 |
| #21 | One dedup set across providers | **Built.** Real cause turned out to be double discovery, not harness mirroring — see the note under #21 |
| #26 | cctop-authored notes per harness | **Built** as `docs/providers/`. Four findings fell out of it — [follow-ups.md](follow-ups.md) |
| — | Subscription burn | **New, and the most interesting thing here.** Not from codeburn at all — see [subscription-burn.md](subscription-burn.md) |

Everything not listed is undecided.

---

## Product ideas

### 1. A waste scanner — `cctop optimize`

Their largest differentiator, and the one cctop is closest to being able to
build. Fourteen detectors run over sessions *and* over `~/.claude/`
configuration: junk reads (`node_modules`, `.git`), the same file read again
across sessions, MCP servers configured but never called, a bloated
`CLAUDE.md`, a low Read:Edit ratio, ghost agents and skills and commands, an
uncapped `BASH_MAX_OUTPUT_LENGTH`, context-ratio anomalies.

Each finding lands in one of three classes, which is the part worth copying:

- **fix** — correctable without a judgement call (archive an unused agent, set
  an environment variable)
- **nudge** — a habit only the user can change
- **keep** — informational, no action implied

and carries an estimated token and dollar saving, a copy-pasteable remediation,
and a label saying whether the saving was **measured** (provider-counted) or
**estimated** (model-derived).

cctop already computes most of the ingredients: `ERR%`, compaction cadence, the
context breakdown with its Unaccounted bar, the Tool Activity panel. This is
the feature that turns numbers already on screen into a sentence about what to
do. The measured/estimated distinction is the same honesty the cost page
already practises.

### 2. Fixes that grade themselves — apply, undo, report

`optimize --apply` walks the findings interactively (`--yes` batches,
`--dry-run` prints the plan). Then:

- `act list` — every change made
- `act undo <id>` — rollback, and it **refuses if the file changed since the
  apply**
- `act report` — after three or more days, estimated saving against realised
  saving

Treating an applied fix as a claim to be checked later, rather than a job done,
is the good idea. It also makes the estimates falsifiable, which is the only
thing that keeps them honest.

### 3. Did the spend ship? — `cctop yield`

Correlate sessions with git commits and classify the spend:

- **productive** — commits from the session landed
- **reverted** — commits later rolled back
- **abandoned** — no commits near the session, or commits never merged
- **ambiguous** — overlapping sessions; the commit goes to the tightest
  containing window

Each commit is attributed to at most one session, and the output prints its own
methodology (`timestamp-window`) so the reader knows it is a heuristic.

cctop can do this better than they can. They only have timestamps; cctop knows
each session's cwd and *which files it edited*, so a commit can be matched by
path overlap rather than by clock proximity. That turns the ambiguous bucket
from a large fudge into a small one.

### 4. Model comparison on your own work — `cctop compare`

Side by side across the models actually in use: one-shot rate, retry rate,
self-correction turns, cost per call, **cost per edit**, output tokens per
call, cache hit rate, and then per-category one-shot rates, delegation rate,
planning rate, tools per turn.

The metric worth lifting on its own is the **one-shot rate**, because of how
they detect a retry: `Edit foo.ts → Bash → Edit foo.ts` is one retry cycle;
editing a *different* file is not a retry. It is file-aware, which makes it a
much sharper waste signal than `ERR%` — a failed call is noise, an agent
editing the same file four times is a story. Their own docs note that file-level
tracking is only possible for Claude, Codex and Goose, and everything else falls
back to tool-name detection; cctop reads the first two at that fidelity already.

### 5. Task classification — thirteen categories, no LLM

Purely deterministic, from tool names and prompt keywords:

| Category | Trigger |
|---|---|
| Coding | Edit, Write |
| Debugging | error/fix keywords alongside tool use |
| Feature Dev | "add", "create", "implement" |
| Refactoring | "refactor", "rename", "simplify" |
| Testing | pytest, vitest, jest in Bash |
| Exploration | Read, Grep, WebSearch with no edits |
| Planning | plan mode, task creation |
| Delegation | subagent spawns |
| Git Ops | git push/commit/merge |
| Build/Deploy | npm build, docker, pm2 |
| Brainstorming | "brainstorm", "what if", "design" |
| Conversation | no tools, pure text |
| General | everything else |

This is cheap, and it is what their whole headline rests on — "half of it went
to conversation instead of code". It also unlocks #1 and #4: a one-shot rate is
only meaningful per category, and a waste finding is only actionable when it
names the kind of work that was wasteful. For cctop it is a new column and a new
panel out of data already parsed.

### 6. Budget caps as hooks — `guard`

Opt-in hooks in `~/.claude/settings.json`: a soft cap (default $5) warns once
in-session, a hard cap (default $15) **stops** the session with `guard allow` to
lift it temporarily, and a checkpoint (default $3) nudges toward a fresh session
if the current one has spent that much and edited or committed nothing. Config
in `guard.json`, `null` disables a cap, and the hooks fail open.

Half of this fits cctop as it stands and half of it collides with a rule.
`src/hook.rs` exits 0 always, by construction, because Claude Code reads a
non-zero exit as a decision to block the tool call. A soft cap and the
spent-but-shipped-nothing checkpoint are just messages and fit fine. A hard cap
is exactly the non-zero exit the module docs forbid. That is a design decision
to make deliberately — probably a separate binary or a separate hook entry with
its own contract — not a thing to bolt onto the existing one.

### 7. Several subscription plans at once

`plan set claude-max` / `claude-pro` / `cursor-pro` / `copilot-pro`, or
`custom --monthly-usd 200 --provider codex`, or `--credits 20000` for Copilot's
credit allotments. They coexist: a Claude Pro and a Codex plan are both active,
and the dashboard shows **one overage row per plan**.

cctop has `--plan max` as a single global flag, and `quota.rs` reading live
usage windows from each provider — which is better data than codeburn has. The
gap is only that a plan is one thing rather than one per provider, and that
there is no explicit "you are $N past the bundle" line.

### 8. Currency

162 currencies, rates from Frankfurter (ECB data, no API key), cached 24 hours,
applied everywhere including exports. `cctop --currency GBP`. Self-contained,
and the 24-hour-cache machinery already exists for LiteLLM pricing.

### 9. Pricing escape hatches

Every one of these is a bug report cctop will eventually receive:

- `price-override <model> --input 0.27 --output 1.10` (USD per 1M tokens)
- `model-alias "my-proxy-model" "claude-opus-4-6"` — for proxies that rename
  the model
- `model-flat-rate <sku>` — mark a subscription SKU so it stops warning about
  a $0 price
- `model-savings "llama3.1:8b" gpt-4o` — price a free local model against a
  paid baseline, for what-if
- `proxy-path ~/work/repo` — mark a checkout as going through a
  subscription-backed proxy
- routing gateways peeled to the upstream model automatically
- hardcoded fallback prices for all Claude and GPT-5 models, so a LiteLLM
  outage cannot silently misprice the common case

All of them support `--list` and `--remove`, which is the right shape for a
config command.

### 10. Where every number came from — `cctop audit`

A per-provider, per-model table of token *sources*. cctop's cost page argues
this in prose; a command that prints it is the machine-checkable version, and it
belongs next to `doctor`.

### 11. Combined totals across machines

`share --pair` prints a PIN, `devices add` finds a machine on the local network,
`devices` shows one combined total. cctop's `--host devbox` already does the
hard part over ssh. What is missing is pairing that does not require ssh config,
and a *combined* figure rather than a merged table.

### 12. A tray presence, and the contract that makes it cheap

macOS `NSStatusItem`, a Windows tray, a GNOME shell extension — all three are
read-only, hold no business logic, and shell out to the CLI to parse
`status --format menubar-json`. The architecture is the lesson: **one stable
JSON output contract, clients update independently of the engine.**

Their clients also validate the argv they spawn against an allowlist and search
only absolute `PATH` directories, because spawning by relative name from a GUI
is a current-directory hijack. Worth knowing before writing any such client.

`cctop serve` already implies a contract of this kind. A `cctop status --format
json` stable enough to build a tray on is the enabling move; the tray itself can
wait for someone who wants it.

### 13. MCP: coarser than cctop's, but with two better instincts

They expose only `get_usage` (fast) and `get_savings` (slow, runs the waste
scan). cctop's four tools are more useful. But:

- **`get_savings` as a tool** lets an agent ask, mid-conversation, whether it is
  being wasteful. That is a genuinely new use for the data.
- **Project names are pseudonymised by default** — stable SHA-256 to
  `project-<6hex>`, absolute paths never exposed, real names only when the
  caller passes `include_project_names: true`. cctop's `list_sessions` and
  `search_sessions` hand real paths to a model that may log them somewhere.
- Every tool declares `readOnlyHint: true`, `idempotentHint: true`,
  `openWorldHint: false`.
- Breakdowns cap at 20 rows by default, because tokens.

### 14. The context browser as a command

They ship `codeburn context` interactively and `codeburn context <id> --json`
for scripting. cctop has the panel; the JSON form is cheap and makes the
breakdown testable from the outside.

---

## Engineering ideas

### 15. Corpus fingerprint plus a settle window

The best technical idea in the set, and it applies to cctop more than to them.

A stat-only pass (`dev`, `ino`, `mtimeMs`, `sizeBytes`) across discovered
session files is the cache key. On a match, serve the persisted payload without
opening anything. On a mismatch, recompute — *unless* the newest mtime is
younger than a settle window (default 2000ms,
`CODEBURN_STATUS_SNAPSHOT_SETTLE_MS`), in which case serve the previous snapshot
and **do not persist**.

The settle window exists because a streaming assistant turn rewrites its file
continuously. Without it, an active session invalidates the cache on every
single poll and the cache never pays for itself. With it, a burst of writes
coalesces into one recompute, and staleness is bounded by the window rather than
being permanent.

They also note that the defer logic belongs in the snapshot layer, not in the
per-file reconciler — pushing it down produced correctness bugs from incomplete
defer signals.

**Result, 2026-09-02: built as `src/fingerprint.rs`, in-process only.**

A stat-only walk folds `(path, dev, ino, mtime, size)` per file into one FNV-1a
hash — the same hash `build.rs` already uses, so no new dependency — sorted
before folding so readdir order cannot move it. `dev` and `ino` come from a
single `identity()` helper that is `#[cfg(unix)]` inside and returns `(0, 0)`
elsewhere, so nothing becomes dead code on Windows. The short-circuit sits at
the top of `load_progressive` and skips discovery, every tail read and every
extraction.

**Measured**, 107 sessions over 575 corpus files, release build: a warm repeat
walk falls from 90–158 ms to **12–14 ms**, of which the fingerprint pass is
8.9 ms. Six to ten times. The cold walk is unchanged, as it must be.

Three things came out differently from the specification, and all three are
improvements on it:

- **The root list is not `watch::roots()`.** Reusing it would have been a silent
  staleness bug, because it omits Gemini and Windsurf — a changed Gemini
  transcript would never have invalidated anything. See
  [follow-ups.md](follow-ups.md); the omission looks like a real bug in `watch`
  independent of this work.
- **A deferral now expires.** A plain settle window can be held open forever by
  a corpus that is never quiet — one agent writing every few hundred
  milliseconds hides every session started beside it — or by a clock-skewed
  mtime that reads as "written just now" permanently. Past five times the
  window the next call recomputes regardless. This is the live-monitor
  constraint asserting itself: codeburn polls for a menu bar, cctop is watching
  something move.
- **No persisted snapshot file, and no query key.** `Session` and its component
  types do not derive `Serialize`, and hand-mirroring a serializable row type
  would recreate exactly the failure `CACHE_VERSION` and `build.rs` exist to
  prevent — a field added upstream and silently missing from the mirror. Within
  one process the query scope is fixed, so a query key would be dead structure.
  The consequence is the inverse of codeburn's: their gain was on the one-shot
  path, ours is on the live one. `--list`, `--json`, `mcp` and `why` walk once
  and exit, and now do not even pay for the reading.

### 16. A snapshot cache keyed by query, not only by corpus

`status-snapshot.<queryKeyHash>.json`, where the query key serialises the
period, provider, project filters, exclusions and flags. A scoped query then
cannot poison a global one. Atomic temp-plus-rename at mode 0600, a numeric
`version` field so a bump forces recompute, and a failed read or write falls
back to a full recompute rather than failing.

cctop already derives its session cache version from the parser sources in
`build.rs`, which is the same instinct done better. The per-query sharding is
the extension.

### 17. Do not fingerprint the cache file itself

Deliberate, and easy to get wrong: `session-cache.json` only rewrites *after*
parsing completes, so including it in the fingerprint makes the key chase its
own tail. The kind of limit a `ponytail:` comment exists for.

### 18. Shard the cache per provider per month

A date-range query then reads only the shards it needs. Their monolith reached
386 MB and re-parsing it was ~20% of CPU time on every invocation — the entire
reason #15 and #16 had to be written.

### 19. Adaptive parallelism, with the decision printed

Parse serially if pending bytes < 200 MB, or cores ≤ 2, or available memory
< 4 GB. Otherwise workers = `min(cores - 1, thread_budget / per_worker,
max(pending_files / 50, pending_bytes / 200MB))`, with a per-worker memory
budget of `clamp(256MB, 2 × (pending_bytes / pending_files) + 128MB, 1GB)`.
`CODEBURN_PARSE_WORKERS=N` overrides every gate and `CODEBURN_VERBOSE=1` **logs
which branch it took**.

Rayon makes the mechanism easier in Rust than their `worker_threads` did. The
transferable parts are the gates — parallelism is a loss on a small corpus — and
the flag that explains the choice, which is the same instinct as `cctop why`.

### 20. Cross-file state stays on one thread

Workers parse against an *empty* dedup set and return their results plus the
keys they claimed; the parent installs them **in order** and owns all shared
state. Replayed keys collide, which is how they make forked Codex rollouts safe
under parallelism — an overlap triggers an in-process re-parse rather than a
wrong answer.

### 21. One dedup set threaded through every provider parser

So a turn appearing in both Claude's log and Cursor's mirror is counted once.
cctop reads seven harnesses and they increasingly mirror one another.

**Result, 2026-09-02: built, but for a different reason than the one above.**
Reading `session::list_all` showed the seven providers concatenated with no
dedup of any kind, and the mirroring codeburn defends against does not happen
between the harnesses cctop reads — no harness copies another's transcript into
its own directory today. What *does* happen is one session discovered twice: a
profile directory nested inside the default one is walked by both, and a home
reachable at two paths (a bind mount, a symlink, `/home/x` also exported as
`/export/home/x`) is scanned as two homes, because `config::OTHER_HOMES` dedups
homes by path and a path is not an identity. Either way the row appeared twice
and its cost landed twice in every total.

`session::dedup` now keys on provider plus session id — `Session::key`, which
already existed — and picks a winner rather than taking the first: this user's
own home beats another home's view of the same session, because `owner` is what
puts a name in the Owner column and mislabelling your own session is worse than
the reverse; failing that the later `last_active` wins. Duplicates dropped are
reported to `--trace`, where zero is the healthy answer and anything else means
two roots overlap. Content-level dedup is explicitly not attempted and is marked
`ponytail:` in the code.

### 22. Lazy provider loading

Their SQLite- and protobuf-backed providers are dynamic imports: thirty
providers load eagerly, eleven only if the tool is present, so a user without
Cursor never pays for `better-sqlite3`. The Rust analogue is a cargo feature per
provider — relevant the first time one harness wants a heavy dependency.

### 23. In-flight coalescing

Concurrent identical requests share one scan rather than racing. Directly
applicable to `cctop serve` with several browser tabs open on the same session.

### 24. Resident process, not exec-per-call

They measured a **17.6 second** warm-start penalty per MCP call before making
the server resident in-process, because every call re-earned a cache that a
fresh process could not inherit. They also pre-warm the `today` dataset at boot.
Worth checking cctop's MCP server against the same trap.

### 25. If team telemetry is ever wanted

OTLP over HTTP JSON, and these specific choices:

- The span ID is **derived deterministically from the dedup key**, so a re-send
  is an idempotent upsert.
- An **exact local sent-ledger** (`sync-ledger.json`, entries pruned at six
  months), not a timestamp watermark — a watermark silently drops late-arriving
  calls.
- Token counts, costs, models and project names travel; code and prompts never
  do.
- Identity comes from the JWT `sub` claim, so no PII rides in the payload.
- OAuth authorization-code with PKCE, a local callback on fixed ports, tokens in
  Keychain / libsecret / DPAPI with a filesystem fallback.
- A discovery document at `/.well-known/codeburn-export.json` gives the client
  the issuer, client ID, scopes and traces path, so the endpoint can move.
- Every attribute past a small core is optional and **sent only when its value
  is proven**, so old receivers ignore what they do not know.

### 26. Their provider documentation layout

One markdown file per provider stating the exact data location, the storage
format, and the known quirks, plus a `NEW_PROVIDER.md` for adding one.

cctop mirrors upstream harness documentation instead, which is a different and
arguably better thing — it survives a format change. But a short cctop-authored
note per harness, saying what we read and where the format lies to us, would
pair well with `doctor`.

**Result, 2026-09-02: built as `docs/providers/`** — seven pages, an index, and
a page on adding an eighth harness. A new directory rather than anything inside
`docs/harnesses/`, because `pull.sh` overwrites each mirror wholesale and a
hand-written note placed there would be deleted on the next refresh; the mirror
argues this itself in its own README.

Every claim is written from the parsers rather than from the mirrored upstream
prose, which was the point, and doing it surfaced four things worth knowing
independently of the docs — see [follow-ups.md](follow-ups.md).

### 27. Two smaller CI notes

Semgrep runs on the hot parsing paths on every push. And three cache-lock test
suites run **serially**, because cross-process file locks contend and the
failures look random. The second is the one that bites later.

---

## The cheapest win: an interpretation table

They publish a table of what a signal might mean:

| Signal | What it might mean |
|---|---|
| Cache hit < 80% | System prompt or context unstable, or caching off |
| Many `Read` calls per session | Agent re-reading, missing context |
| Coding one-shot rate ~30% | Retry loops |
| A large model on small turns | Overpowered for the work |
| Subagent-heavy | Fan-out, expected or excessive |
| No MCP usage | Unused, or the config is broken |
| Bash dominated by `git status`, `ls` | Exploring instead of executing |
| Conversation category dominant | Talking instead of coding |

With the caveat attached: one session at 60% cache hit is normal, 60% every week
is a configuration problem.

cctop already puts most of these numbers on screen. A page — or a `?` overlay
per column — saying what a bad value means costs a day and changes what the
table is *for*. It is the smallest change here with the largest effect on how
the tool reads.

---

## Opinion

If three of these get built, they should be:

1. **The settle-window fingerprint cache (#15–#17)** — pure engineering, no
   product risk, and cctop polls a live tree constantly, which is precisely the
   case that defeats a naive mtime cache.
2. **Task classification (#5)** — cheap, deterministic, and it is the
   prerequisite for the whole optimize/compare family.
3. **File-aware one-shot rate (#4)** — a better waste signal than `ERR%`, and
   the data to compute it is already parsed for Claude and Codex.

And #12's stable JSON contract is worth settling early, because it is much
harder to introduce once something depends on the current shape.

The one to be careful with is **guard (#6)**: the hard cap wants a non-zero exit
from a hook that is documented never to produce one. That is a real decision,
not an implementation detail.

---

## Two smaller things

### 28. Attribution for prior art, not only for dependencies

Their `THIRD_PARTY_NOTICES.md` gives each entry the *context of use* rather than
a bare licence line, pins vendored assets to a revision, carries a trademark
disclaimer for provider logos, and — where an upstream declares two conflicting
licences (BSD 3-Clause on npm against MIT in the monorepo) — reproduces the
stricter one. It also credits **CodexBar** as prior art for the provider usage
tracking, though it is not a dependency.

cctop mirrors 14 MB of other people's documentation under `docs/harnesses/` and
`docs/rmux/`. The mirroring is deliberate and explained in `Cargo.toml`, but the
terms under which those copies are held are not written down anywhere. Worth a
notices file of the same shape.

### 29. CodexBar — a lead, not yet read

Named in their notices as the prior art behind provider usage tracking. Reading
it is the obvious next research task after this one, since provider quota
reading is something cctop already does in `quota.rs`.
