# In a browser

```bash
cctop serve
```

```
cctop: serving on http://127.0.0.1:7777/?t=9f3ac1de…
cctop: read-only link — no actions: http://127.0.0.1:7777/?t=71b02ee4…
```

Open that link and you get the table cctop draws in the terminal, streamed live,
plus what the terminal has no room for: per session, the conversation itself,
what it edited, what it can reach, a report that says where an afternoon's money
went, and each account's rate-limit windows at `/api/quota`.

**It can also answer an agent.** A page that tells you a session has been waiting
twenty minutes and cannot do anything about it has shown you a problem and
withheld the fix, so the page can send a prompt to a live session, resume a dead
one, and hand one's work to a different harness. Nothing destructive: no route
stops an agent, kills a process or deletes a transcript — those stay in the TUI,
where the confirmation prompt is.

The whole authorisation for that is the token in the URL, so it is worth stating
plainly: **whoever holds the link can drive the agents on this machine.** Hence
the defaults — loopback, a token, and `--no-actions` to serve the pages without
the buttons.

**The second link is for showing, not driving.** Each run also mints a read-only
token, printed as a labelled second line. It opens every page and answers every
`GET` — the same dashboard, the same reports, the same search — but every
`/api/act/*` request it makes is refused with `403 this link is read-only`, and
the pages it serves are wired to it, so the prompt box never appears. It is a
different credential rather than a flag on the first one: "read-only" is not
something a request can assert about itself, it is a property of the token it
carries. Hand it to a colleague who wants to watch a run, or a status board that
has no business typing at your agents.

## From inside cctop, with the table still there

`cctop serve` takes over the terminal it runs in and draws nothing — which is
the wrong shape when you want the page *and* the dashboard. **`B` in the TUI
serves the same page while cctop keeps running.** `l` puts it on this machine,
`t` also opens a tunnel, `o` opens it in your browser, `y` copies the link, and
`x` stops it. With a tunnel up, `c` draws the tunnel link as a QR code for a
phone to scan.

```
╭ Serve this table to a browser ─────────────────────────────╮
│ This machine                                               │
│  http://127.0.0.1:7778                                     │
│                                                            │
│ The internet                                               │
│  https://supplemental-belt-spare-reflect.trycloudflare.com │
│                                                            │
│ Anyone holding it reads every session here,                │
│ and can type at your agents. Cloudflare carries it.        │
│                                                            │
│ Read-only — watches, never acts                            │
│  https://supplemental-belt-spare-reflect.trycloudflare.com │
│                                                            │
│ o open · y copy · c QR code · l local · t + tunnel · x stop│
╰────────────────────────────────────────────────────────────╯
```

The panel shows each link as its origin and not in full, because the full link
carries the token — a credential that would otherwise be sitting in every
screenshot of the panel. `o` and `y` use the whole thing, and so does `c`: a QR
code is the link in a form a camera reads, so it waits to be asked for, goes
when the panel closes, and is not drawn at all on a terminal too small to hold
all of it — the panel says so instead.

It is the same server, reached differently, with one difference worth knowing:
it does not scan for sessions. The dashboard already walks them several times a
second, so the page is fed from the rows on screen — one pass over the disk
instead of two, and no way for the page and the table beside it to disagree.

Stopping it, or quitting cctop, revokes every link handed out. A tunnel lasts
exactly as long as the cctop that opened it.

## Why you would want it

The table's headline is *which session is waiting on you*, and that only pays off
while somebody is looking at the terminal. Agents sit amber for twenty minutes
because the person who could answer them is in a meeting. `cctop serve` puts the
same fact on a surface you carry.

Sessions that need a person are pulled out into their own block at the top of the
page. The **Notify** button asks for browser notification permission and then
fires one the moment a session *crosses into* waiting — not for the ones already
sitting there when you opened the page, which would be a notification about
nothing.

The same edge can leave the browser entirely. `--notify <URL>` (or
`CCTOP_NOTIFY_URL`, which a dashboard-hosted serve also reads since it has no
flag) POSTs one JSON event per crossing:

```json
{
  "event": "waiting",
  "session_id": "…",
  "project": "cctop",
  "title": "…",
  "url": "https://…/session/…?t=…"
}
```

`event` is `waiting` or `asking`; `url` links the session page, on the tunnel
origin when there is one so the link works from wherever the webhook lands.
Edge-triggered means exactly that: the first snapshot fires nothing, a session
sitting waiting fires once rather than once per refresh, and it fires again only
after it leaves the state and re-enters. The POST is made on a spawned thread
with a five-second deadline, and a dead endpoint is reported once on stderr and
then stays silent — a webhook that never answers costs the refresh loop nothing.

## The report

Click any row, or go straight to `/session/<id>` (any unambiguous prefix of the
id works, so a link is short enough to paste into chat).

The report is built from a full transcript read, which is why it happens for one
session on request rather than for all of them on a timer. It answers three
questions, in the order they are usually the answer:

**What failed, repeatedly.** Failed tool calls are grouped by tool *and by the
argument they were called with*. Eleven failures of `Bash` is a session having a
bad afternoon; eleven failures of `Bash` running the identical command is a loop,
and the agent paid for every attempt. Every per-tool error count there is renders
those two identically. This one does not.

**Where the context window went.** The stacked bar says what is in the window
now — startup, tool output, attachments, and an Unaccounted slice that never
pretends to be smaller than it is. The chart under it says how it got there, with
compactions marked: a window that climbed steadily is a conversation growing, and
one that jumped is a single tool result that will do it again.

**What it cost, split by model.** Plus spend per hour, the slowest individual
calls by wall time, the full tool table, and the files the session wrote.

## Reaching it from your phone

The default bind is `127.0.0.1`, so out of the box nothing but this machine can
open it. Two ways to go further, in the order you should prefer them:

**A tunnel you already have.** Tailscale, or plain ssh:

```bash
ssh -L 7777:127.0.0.1:7777 devbox
```

Now `http://127.0.0.1:7777` on the laptop is the devbox's cctop, authenticated by
ssh, encrypted by ssh, and exposed to nobody.

**Or a tunnel cctop opens for you**, when there is no ssh to hand and the phone
is not on the same network:

```bash
cctop serve --tunnel
```

```
cctop: opening a trycloudflare tunnel…
cctop: serving on https://particular-words-here.trycloudflare.com/?t=9f3ac1de…
cctop: also on http://127.0.0.1:7777/?t=9f3ac1de…
cctop: read-only link — no actions: https://particular-words-here.trycloudflare.com/?t=71b02ee4…
cctop: that first link is on the public internet. Anyone who has it can read
       every session on this machine — and, unless --no-actions, type at your
       agents, which runs commands as you. Cloudflare carries the traffic and
       can read it. The tunnel ends when this process does.
```

This needs nothing installed: cctop speaks the trycloudflare protocol itself
rather than shelling out to `cloudflared`, and the tunnel's data path runs inside
the cctop process, landing on the same loopback listener a local browser uses. So
the token, the connection cap and the request deadlines all still apply — the
tunnel adds a route in, not a second server.

What it is not is a private channel. Cloudflare terminates the TLS, which is
worth saying because the opposite is easy to assume of anything with an `https://`
URL. The hostname is new every run, it stops existing when cctop does, and
`--tunnel` refuses `--no-token` outright: a public URL with no token is a prompt
box for your agents that anyone who finds it can use.

**Or bind wider**, and understand what that means:

```bash
cctop serve --bind 0.0.0.0
```

```
cctop: serving on http://0.0.0.0:7777/?t=9f3ac1de…
cctop: this is reachable from your network — anyone who can open that link can
       read every session on this machine.
```

cctop terminates no TLS of its own on that socket. A loopback listener does not
want it, and doing it for the LAN case means certificates cctop has no business
managing — which is why an ssh tunnel is still the better answer here, since it
authenticates rather than merely encrypting. `--tunnel` gets its `https://` by
borrowing Cloudflare's certificate and Cloudflare's edge, which is a different
trade, not a stronger one.

## The token

Each run generates an access token and puts it in the URL it prints. Every
request needs it, including the ones that only return HTML, so a wrong token
cannot be used to find out which routes exist. The one exception is
`GET /metrics`, which carries no transcript and answers without one — see
[Scraping it](#scraping-it-with-prometheus).

The URL being the whole credential is deliberate: it makes the link shareable
over whatever channel you already trust, and it is why `--no-token` is a thing to
justify rather than a convenience. Without it, every process and every user on
the machine can read your sessions — and a later `--bind` exposes them to the
network with no gate at all.

Restarting `cctop serve` mints a new token and invalidates the old link —
unless `--token-file` keeps them, [below](#keeping-them-across-restarts).

Either token is accepted in three places, all checked by the same gate: the
`?t=` query parameter a link carries, the cookie a page hands back so a reload
still gets in, and an `Authorization: Bearer <token>` header — for a script or
an HTTP client, and it keeps the token out of the URL they log and display.

### Keeping them across restarts

A token minted per run is right for a link that can type at your agents, and
wrong for a bookmarked dashboard or a read-only link pinned to a status board,
which then break on every restart. `--token-file` is the opt-in:

```bash
cctop serve --token-file ~/.config/cctop/serve-tokens
```

```
cctop: new tokens written to /home/you/.config/cctop/serve-tokens — they outlive this run; --rotate-token replaces them
```

When the file exists its tokens are served; when it does not, fresh ones are
minted and written to it — created with `O_EXCL` and mode `600`, so there is no
moment at which they sit in a file someone else can read. The next run says
`tokens from …` and every link from the last one still works.

A file that is not plainly yours is refused rather than repaired:

- **readable or writable by group or others** — anyone who could read it
  already holds the credential, and tightening the mode now does not un-leak it;
- **owned by another user** — they can rewrite it, and a token someone else
  chose is one they know;
- **a symlink, or not a regular file** — the file checked has to be the file
  read, and a link can be repointed in between.

Each refusal says so and names the way out. **Rotating** is
`cctop serve --token-file <path> --rotate-token`, or deleting the file: both
mint new tokens and revoke every link built on the old ones. The directories above the file are not checked, so put it somewhere only
you can write.

The dashboard's `B` never reads a token file: stopping that serve still revokes
every link it handed out.

## Several machines at once

`--host` works the same as it does in the terminal, and composes with everything
above:

```bash
cctop serve --host devbox --host build-01
```

Remote rows are read over ssh and merged into the same table. A host that cannot
be read shows a banner saying which one and why, rather than quietly going
missing and leaving the totals looking complete.

A remote row's report page works too: `/api/report`, `/api/chat` and
`/api/access` are answered by asking the cctop on the machine the session lives
on — `ssh <host> cctop --report <id>` over the same channel the rows arrive by —
and relaying the document it prints. Nothing is parsed at the row's path on
this filesystem, which would report whatever happens to live there locally.
When the far side cannot answer — the host is down, or its cctop is older than
these flags — the route says so with a 502 rather than an empty page.

## Flags

| | |
|---|---|
| `--bind <ADDR>` | Address to listen on. Default `127.0.0.1` |
| `--port <PORT>` | Default `7777`. Without this flag a busy port is stepped past; with it, a busy port is an error |
| `--no-token` | Serve with no access token. Also turns actions off — the token is what authorises one |
| `--no-actions` | Serve the pages without the buttons: no prompts, no resuming, no handoff |
| `--tunnel` | Also reach the page over a trycloudflare quick tunnel. Refuses `--no-token` and `--bind` |
| `--plan <PLAN>` | `retail`, `max` or `included`, as elsewhere |
| `--delay <SECS>` | Seconds between refreshes. Default `2` |
| `--host <HOST>` | Also serve another machine's sessions. Repeatable |
| `--notify <URL>` | POST a JSON event when a session crosses into waiting or asking. Also read from `CCTOP_NOTIFY_URL`, including by a dashboard-hosted serve |
| `--token-file <PATH>` | Keep the tokens across restarts: read them from PATH, or mint them and write it with mode `600`. Refuses a file others can read or do not own, and `--no-token` — see [the token](#keeping-them-across-restarts) |
| `--rotate-token` | With `--token-file`: replace the tokens in it, revoking every link built on the old ones |

## What it serves

| | |
|---|---|
| `GET /` | The dashboard |
| `GET /session/<id>` | The report page for one session |
| `GET /api/sessions` | The same document `cctop --json` prints |
| `GET /api/report/<id>` | The report, as JSON |
| `GET /api/events` | Server-sent events; one `sessions` event per refresh |
| `GET /api/hosts` | Which `--host` machines could not be read, and why |
| `GET /api/quota` | Each account's rate-limit status and windows — `{"claude":[…],"codex":[…]}`, each profile with `status`, `detail`, `plan` and `windows` |
| `GET /api/search?q=<query>` | Both search tiers over every session: `{"hits":[{key, session_id, snippet, score}]}`. Literal matches carry the matching text; topical-only hits carry `~NN% ` plus the chunk head |
| `GET /metrics` | The same table as Prometheus text exposition. **Needs no token** — see [Scraping it](#scraping-it-with-prometheus) |
| `GET /insight/optimize` | The text `cctop optimize` prints, as `text/plain` |
| `GET /insight/compare` | The text `cctop compare` prints, as `text/plain` |
| `GET /favicon.svg` | The page's icon |
| `GET /manifest.webmanifest` | Installable-page metadata, so a browser can add the dashboard as an app |

`/api/sessions` is byte-for-byte the document `--json` prints and `--host` parses
— one builder, so a browser is never shown different figures than the terminal.

`/api/quota` is served from memory: a standalone `serve` polls the usage
endpoints itself on a 60-second cadence — slower than the two-second session
refresh, because they throttle hard — and a dashboard-hosted one serves the
reading the TUI's own poller already made. Either way a request never triggers a
fetch.

`/api/search` runs the same two tiers the TUI's `s` does: the literal byte scan,
then the embedding index when the model is installed and the query earns it —
an empty literal answer, or a sentence-length question. Queries under three
characters answer `{"hits":[]}`, hits are capped at 25, and a missing or
unloadable index simply means the literal tier answered alone.

The insight routes re-parse every transcript on request, the same cost the CLI
pays — they are asked for, not polled.

## Scraping it with Prometheus

`/metrics` is the snapshot every other route serves, summed into gauges in the
Prometheus text format. It says nothing `/api/sessions` does not, and leaves
out titles, prompts and full paths on purpose, since a metrics store keeps what
it is sent for months and shows it to anyone with a dashboard login.

**It is the one route that needs no token.** What is left is aggregate counts,
costs, model names, project directory names and eight-character session ids —
nothing a token would be protecting — and a scrape config is the file that gets
copied into config repositories and Helm values, which is the last place a
credential that also opens your transcripts should live. It also means a
restart never breaks the scrape. Every other route keeps its check, and only
`GET` (or `HEAD`) is answered. The cost: whoever can reach the port can read
the numbers — this machine's users on the default loopback bind, your network
under `--bind`, and anyone with the hostname under `--tunnel`.

| Metric | Labels | |
|---|---|---|
| `cctop_build_info` | `version` | Always 1 |
| `cctop_remote_hosts_unreadable` | | `--host` machines that could not be read |
| `cctop_sessions` | `provider`, `state` | `state` is `working`, `waiting`, `asking`, `error`, or `idle` for a session with no live process. Every state is present, at 0 when empty, so an alert on `waiting` always evaluates |
| `cctop_cost_usd` | `provider`, `included` | Estimated cost at retail rates of every session in the table |
| `cctop_cost_today_usd`, `cctop_cost_this_hour_usd`, `cctop_cost_last_hour_usd` | `provider`, `included` | Since local midnight, in the current local clock hour, and in the last 60 minutes rolling |
| `cctop_cost_burn_usd_per_hour` | `provider`, `included` | The smoothed live spend rate |
| `cctop_model_cost_usd`, `cctop_model_cost_today_usd` | `provider`, `model`, `included` | The same, by model |
| `cctop_tokens` | `provider`, `model`, `kind` | `kind` is `input`, `output`, `cache_read` or `cache_write` |
| `cctop_tool_calls`, `cctop_tool_call_errors` | `provider` | Errors only for providers whose transcripts record a per-call outcome — a zero elsewhere would claim "nothing failed" |
| `cctop_context_used_tokens`, `cctop_context_max_tokens`, `cctop_context_fill_ratio` | `session`, `project`, `provider` | **Live sessions only** |

Cost is reported whatever the plan, with `included="true"` marking usage the
plan bundles — a retail equivalent rather than money spent, as the analytics
page labels it `incl`.

Everything is a gauge, even the totals. They are sums over the sessions in the
table, and a session can leave it — a host goes dark, a transcript is deleted —
which a counter would report as a reset and `rate()` would turn into a spike.

**Cardinality is bounded.** Labels take values from small, closed sets —
provider, state, model, token kind. The one exception is the context window,
which only means anything per session: those series carry the id's first eight
characters and the project's directory name, and exist only while the session
is live. A session that ends drops out of the next scrape instead of leaving a
series behind for every session ever run.

A remote row's transcript is not on this machine, so its tokens and cost are
filed under the model it is on, with its input kinds folded into `input`.

```yaml
scrape_configs:
  - job_name: cctop
    scrape_interval: 30s
    static_configs:
      - targets: ["127.0.0.1:7777"]
```

No `authorization:` and no `params:` — a plain `cctop serve` is scrapeable as
it is, and stays so across restarts.

## Notes

The pages carry their own CSS and JavaScript and fetch nothing at all, which is
what lets every response send `default-src 'none'`. The only same-origin
exceptions are `img-src 'self'` for the favicon and `manifest-src 'self'` for
the manifest — the two requests an installable page cannot inline. There is no
build step, no CDN, and no assets on disk: an installed cctop is one binary, and
a page that loaded its own stylesheet would break the moment that binary moved.
