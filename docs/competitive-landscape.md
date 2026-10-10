# Competitive landscape

Every tool found in the same waters as cctop, with what it does, what it
doesn't, and what cctop might take from it. Surveyed October 2026.

## Where cctop sits

cctop is the intersection of three layers the others each own one of:

- **Observability** — read the harness's own logs and show cost, cache,
  context, subagents (what claude-monitor and codeburn do).
- **Control** — know which session needs you, answer it, drive it
  (what herdr, nsq, swe-mux do).
- **Remote access** — the same sessions from a phone, token-gated
  (what nsq and swe-mux do, differently).

No other tool combines all three in one Rust binary with a TUI *and* a web
dashboard. On top sit things nobody else has together: the **issue loop**
(GitHub issues as a solver queue with labels and draft PRs), **insights**
(per-session optimize findings), **MCP tools**, **subagent trees**, and
per-harness parsers for Claude Code, Codex, OpenCode, Cursor, Copilot and
Windsurf.

## 1. Agent runtimes and multiplexers

These own the terminals the agents run in. cctop is adjacent: it observes
their sessions, it does not host them.

### herdr — <https://github.com/herdrdev/herdr>

~41k stars · Rust · Apache-2.0 · the big one.

A background server owns agent terminals; close the client or lose SSH and
the agents keep working. After a restart it restores layout and can resume
sessions. tmux-style prefix keys *and* mouse (click, drag, split) both
first-class. Every pane marked working / blocked / idle, and when an agent
stops needing an answer, herdr says so.

- **Agent-native** — the CLI and socket API are one surface: agents spawn
  panes, prompt each other, wait until another agent is genuinely blocked.
  Ships an agent skill for it.
- Runs Claude Code, Codex, Cursor, OpenCode, Grok — owns their terminals,
  doesn't wrap them.
- Plugin marketplace (a Cloudflare worker serves it).
- One Rust binary, no Electron.

**For cctop:** the working/blocked/idle tri-state and the agent-to-agent
prompting API are the two ideas worth watching. cctop's state column is
already richer (needs-input, perms, cost), but "wait until genuinely
blocked" is a coordination primitive cctop's issue loop only approximates.

### nsq (NeuroSquad CLI) — <https://github.com/glmn-ai/neurosquad-cli>

27 stars · Node · MIT · preview (0.1.0).

Run several agents from one terminal; a per-user daemon owns them, a grid
dashboard shows live terminals, and the agent that needs you carries *the
question it is asking*.

- **Exact status from hooks, not screen-guessing.** "Needs you" includes the
  harness's own text ("Claude wants to run: npm test").
- **Answer inline** — `y` / `a` / `n` on a permission prompt, `i` interrupt,
  `s` send a prompt, `S` send when the turn ends. All scriptable
  (`nsq answer`, `nsq send`).
- **Notifications** — native OS notification per agent, withdrawn when it
  works again; bell + OSC 9 fallback over SSH.
- **Phone** — QR pairing, LAN or Cloudflare quick tunnel; from the phone:
  see screens, send prompts, answer permissions, interrupt. Cannot start
  agents or change settings. Push via ntfy.
- **Voice dictation** — local model (~670 MB, SHA-256-pinned), pasted never
  sent.
- Worktree per agent (`--worktree`, branch `nsq/<name>`), `nsq diff`.
- Cost per agent from each harness's own logs; unknown price shows "no
  price", never $0.
- OpenRouter built in; own model servers (llama.cpp, Ollama, vLLM…);
  keys only in env/keyring, never argv.
- Auto-update: background install, switch when nothing would be lost.

**For cctop:** the strongest reference for the *control* layer. Steal:
(1) the question text travelling with "needs you" — cctop shows that a
session needs input but not always *what* it asked; (2) `y/a/n` answering
from the dashboard; (3) OS notifications withdrawn on state change;
(4) ntfy push to phone; (5) "no price" rather than a fake $0.00 (cctop
already has `cost_is_free` for subscriptions — same instinct).

### swe-mux — <https://github.com/jatoran/swe-mux>

8 stars · Python · Apache-2.0 · DCO.

Terminal multiplexer + agentic control plane, phone-first over Tailscale.

- **Tailscale is the access boundary** — no separate remote login; a tailnet
  peer the policy admits has terminal authority. `0.0.0.0`, LAN binding,
  Funnel all explicitly unsupported.
- Desktop (Windows-first) + browser; the same live sessions on the phone:
  terminals, git review, files, queues, previews, push alerts.
- **Land queue** — reconcile a branch, run approved checks, fast-forward;
  conflicts and failed checks return to the owning agent. (A merge queue
  cctop's issue loop stops short of — cctop never merges.)
- Shared editor behavior, keymaps (tmux/VS Code/Vim/Emacs), clipboard image
  paste.
- Voice: local speech recognition, read-aloud, assistant that inspects
  sessions under trust settings.
- Cross-agent conversation search; transcript reopen; optional recorded
  evidence and commit provenance.
- Provider account switching (one person, own accounts, explicit).
- Quiet hours; separate Desktop/Mobile notification profiles.

**For cctop:** (1) quiet hours and per-device notification profiles;
(2) searchable alert history; (3) Tailscale-vs-tunnel comparison — cctop
picked Cloudflare (share links, Access), swe-mux picked Tailscale; their
docs argue the boundary clearly and ours could too; (4) the land queue is
what our issue loop grows into if merging ever leaves the user's hands
(it shouldn't, but the design is there).

### herdash — <https://github.com/heysanil/herdash>

Terminal dashboard *for herdr*: lifecycle status per agent grouped by repo,
plus an **LLM-written summary** of each agent's terminal output — task, now,
recently finished.

**For cctop:** an "LLM summarizes this session" one-liner per row (opt-in,
costs a cheap call) would be a new insight type. The data is already there.

### herdr-orchestrator — <https://github.com/kylezk777/herdr-orchestrator>

File-based orchestrator-operator coordination on top of herdr. Niche, but a
proof that "coordinate agents without watching terminals" is a layer people
are building *on* runtimes.

## 2. Cost and usage observability

These read the harness logs and print money. cctop's cost pages cover this
ground — the differentiators below are what they do that cctop doesn't.

### ccgauge — <https://github.com/chengzuopeng/ccgauge>

22 stars · MIT · web dashboard, `npx ccgauge`, everything local.

Claude Code + Codex side by side, switch or merge; day/project/model/session
breakdowns; dollar cost; terminal report with `ccgauge report -d`; own MCP
server (9 tools: summary, time-series, breakdowns, cost estimator).

- **Cache savings as a first-class KPI** — dollars Anthropic prompt caching
  saved this week, a card on the overview, not a footnote.
- **Live 5h block countdown** — Claude rate-limit window as a gauge.
- Offline mode: committed price snapshot, `CCGAUGE_OFFLINE=1`.
- Reasoning-token breakdown; light/dark; EN/中文.

**For cctop:** (1) cache-savings-as-dollars on the *overview*, not just the
optimize page — we compute it, we bury it; (2) the 5h block as a countdown
gauge; (3) a "cost estimator" tool in our MCP surface ("what would this
session cost on Opus?").

### claude-monitor (Zxela) — <https://github.com/Zxela/claude-monitor>

Go + Vite · brew tap · SQLite history at `~/.claude-monitor/history.db`.

Real-time web dashboard: live cost, token usage, cache hit rates, tool
execution feeds, session replay.

- Four views: list (cards + live feed), **graph** (force-directed agent
  dependency graph on Canvas), table (sortable), history (SQLite-backed).
- Per-session cost rate ($/min), duration, errors; global weighted cache %.
- Cross-session search, highlighted, grouped by session.
- Optional hook install to auto-start.

**For cctop:** (1) the **agent dependency graph** view — subagent trees
per session exist in cctop; a force-directed *cross-session* graph is the
next step; (2) $/min cost rate as a column; (3) SQLite persistence of
history — we recompute from source logs every time.

### agent-trail — <https://github.com/camtrik/agent-trail>

12 stars · five harnesses (Claude Code, Codex, OpenCode, OpenClaw, Qoder).

Session replay with tool calls expanded (exact input, full output the model
received) and **subagent trees rendered nested**. Local session-search agent
skill: the agent finds your past sessions through a local API without you
knowing the session ID.

**For cctop:** (1) our subagent panel is a table; theirs is a *tree* — nested
rendering would show hierarchy the flat list hides; (2) the search skill —
"cctop find …" as an MCP tool the model can call to search history.

### codeburn — <https://github.com/acumenix/codeburn>

Interactive TUI cost observability, seven providers (Claude Code, Claude
Desktop, Codex, Cursor, OpenCode, Pi, Copilot). Already benchmarked in
detail earlier: 162 currencies, flat-rate plans, proxy paths, fallback
prices, per-turn task classification, `codeburn share --pair` for combined
totals across machines.

**For cctop:** the previously agreed top steals stand: currency support,
pricing escape hatches (`price-override`, `model-flat-rate`), the
interpretation table (what a bad number means), per-turn classification.

### opencode-bar — <https://github.com/opgginc/opencode-bar>

Menubar quota tracker, ~12 providers auto-detected from OpenCode config
(OpenRouter, Zen, Copilot, Claude, MiniMax, Grok, Z.AI, Brave, Tavily…).
Color-coded progress, JSON out, exit codes for CI gating.

**For cctop:** the provider-quota catalog idea — cctop tracks *session* cost;
this tracks *account* quota across providers. A `quota` panel or column
("Claude: 60%, 100%") is the piece missing from cctop's subscription view.

### opencode-usage — <https://github.com/gaboe/opencode-usage>

CLI + "Commander" web dashboard: daily/monthly breakdown, provider filter,
multi-account quota bars with configurable thresholds and stale detection.

### ccusage & friends

`ccusage` (ryoppippi) is the granddaddy the others compare against — prints
usage tables. Around it: **claude-session-monitor** (emssik, Max-quota
dashboard, anti-flicker, macOS notifications), **claude-usage-dashboard**
(wcruz, one script against the undocumented OAuth usage endpoint),
**opencode-cost-monitor** (mikecase, live 3s polling, cache economics,
Go-vs-Zen plan pricing), **metric-dashboard** (intisy, OpenCode+Claude
credits, Firebase cross-device sync), **ccdash** (jedarden, Go Bubble Tea
TUI: tokens + hook states + system panel).

**For cctop:** individually thin, collectively a wall — the space is
crowded at the "print tokens" layer and empty at the "act on it" layer.
cctop's bet is the right one: every one of these shows a number; none of
them can kill the session burning it.

### claude-monitor (szaher) — <https://github.com/szaher/claude-monitor>

Hook-based: every session, message, tool call into local SQLite; web UI for
dashboards, analytics, live activity.

## 3. Adjacent layers

### sessionport — <https://github.com/azizmass/sessionport>

5 stars · MIT · port sessions between Claude Code ↔ OpenCode ↔ Codex.

Canonical SessionIR (messages, parts: text/reasoning/tool_call/tool_result),
as-is or compacted modes, export to JSON/markdown/seed-prompt, **import
straight into the other tool's database** (OpenCode SQLite with backup +
transaction; Claude JSONL; Codex rollouts). `sessionport plan` ports just
the plan from plan mode, titled after its heading.

**For cctop:** not a competitor — a companion. cctop could shell out to
it for a "resume this session in another harness" action, or reimplement
the IR read for a unified history view.

### opencode-honcho — <https://github.com/plastic-labs/opencode-honcho>

Memory/personalization layer: Honcho's theory-of-mind memory wired into
OpenCode sessions.

**For cctop:** the "memory across sessions" layer cctop doesn't touch and
probably shouldn't — but session *summaries* (herdash's LLM-written blurb)
are the lightweight cousin.

### claude-history-viewer

Windsurf marketplace extension: browse/search/diff/resume/archive sessions
across Claude Code, Codex, OpenCode, Grok, Copilot; token/cost/quota
dashboard. A UI on the same data, inside the editor.

## 4. Idea backlog, ranked

What the field has that cctop doesn't, by daily value ÷ build size:

1. **Desktop notifications when a session needs you** (nsq, swe-mux,
   claude-monitor, herdr all have it) — withdrawn when it recovers. Small:
   the states are already computed.
2. **Quota windows** — Claude 5h/7d, Codex weekly, as gauges (opencode-bar,
   ccgauge). cctop shows API cost but not "how much subscription is left".
3. **Answer permission prompts from the dashboard** (nsq `y/a/n`) — the
   TUI can drive; the web/share page mostly can't.
4. **Push to phone** — ntfy/FCM when an agent needs you (nsq). The tunnel
   serves the page but nothing pings you.
5. **Question text travels with needs-input** (nsq) — show *what* the agent
   asked, not just that it asked.
6. **Cache-savings dollars on the overview** (ccgauge) — compute it, stop
   burying it.
7. **Cross-session search** (claude-monitor, swe-mux) — and an MCP tool
   for it so agents can search history (agent-trail's skill).
8. **Session replay with expanded tool calls** (claude-monitor,
   agent-trail) — we have the transcript data.
9. **Subagent tree as a nested tree**, not a flat table (agent-trail).
10. **$-per-minute cost rate column** (claude-monitor).
11. **Agent dependency graph view** (claude-monitor) — force-directed
    across sessions.
12. **Quiet hours + per-device notification profiles** (swe-mux).
13. **LLM one-line session summaries** (herdash) — opt-in, cheap model.
14. **"No price" instead of $0.00** for unknown models (nsq) — don't fake
    certainty.
15. **SQLite persistence of computed history** — stop recomputing from
    source logs on every run.
16. **Cost estimator MCP tool** — "what would this cost on model X?"
17. **Tailscale as an alternative access boundary** — documented trade-off
    vs the Cloudflare tunnel, not a feature.

## 5. Themes worth noticing

- **The control layer is winning.** herdr's 41k stars, nsq's exact-status
  hooks, swe-mux's land queue — the market is moving from "show me tokens"
  to "tell me which agent needs me and let me answer from anywhere".
- **Hooks beat screen-scraping.** Every serious tool takes status from the
  harness's own lifecycle hooks, never from parsing the visible screen.
- **Phone is table stakes.** nsq (QR+tunnel), swe-mux (Tailscale), herdr
  (reattach over SSH) — remote access is assumed, not a bonus.
- **Local-only is a selling point.** Every tool in section 2 leads with
  "your data never leaves your machine". cctop's share page must keep
  earning that claim (see the hardening work in flight).
- **Nobody merges.** swe-mux's land queue is the only automated-merge
  design found; everyone else leaves the merge to the human. cctop's issue
  loop matches the market here, deliberately.
- **Rust and single binaries are a moat.** herdr and cctop are the Rust
  entries; the rest are Node or Python with heavier installs.
