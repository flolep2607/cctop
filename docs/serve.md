# In a browser

```bash
cctop serve
```

```
cctop: serving on http://127.0.0.1:7777/?t=9f3ac1de…
```

Open that link and you get the table cctop draws in the terminal, streamed live,
plus what the terminal has no room for: per session, the conversation itself,
what it edited, what it can reach, and a report that says where an afternoon's
money went.

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

## From inside cctop, with the table still there

`cctop serve` takes over the terminal it runs in and draws nothing — which is
the wrong shape when you want the page *and* the dashboard. **`B` in the TUI
serves the same page while cctop keeps running.** `l` puts it on this machine,
`t` also opens a tunnel, `o` opens it in your browser, `y` copies the link, and
`x` stops it.

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
│ o open · y copy · l local · t + tunnel · x stop            │
╰────────────────────────────────────────────────────────────╯
```

The panel shows each link as its origin and not in full, because the full link
carries the token — a credential that would otherwise be sitting in every
screenshot of the panel. `o` and `y` use the whole thing.

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
cannot be used to find out which routes exist.

The URL being the whole credential is deliberate: it makes the link shareable
over whatever channel you already trust, and it is why `--no-token` is a thing to
justify rather than a convenience. Without it, every process and every user on
the machine can read your sessions — and a later `--bind` exposes them to the
network with no gate at all.

Restarting `cctop serve` mints a new token and invalidates the old link.

## Several machines at once

`--host` works the same as it does in the terminal, and composes with everything
above:

```bash
cctop serve --host devbox --host build-01
```

Remote rows are read over ssh and merged into the same table. A host that cannot
be read shows a banner saying which one and why, rather than quietly going
missing and leaving the totals looking complete.

Remote rows have no report page: the transcript is on the other machine, and
parsing the same path here would report whatever happens to live at it locally.
Run `cctop serve` there for that.

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

## What it serves

| | |
|---|---|
| `GET /` | The dashboard |
| `GET /session/<id>` | The report page for one session |
| `GET /api/sessions` | The same document `cctop --json` prints |
| `GET /api/report/<id>` | The report, as JSON |
| `GET /api/events` | Server-sent events; one `sessions` event per refresh |
| `GET /api/hosts` | Which `--host` machines could not be read, and why |

`/api/sessions` is byte-for-byte the document `--json` prints and `--host` parses
— one builder, so a browser is never shown different figures than the terminal.

## Notes

The pages carry their own CSS and JavaScript and fetch nothing at all, which is
what lets every response send `default-src 'none'`. There is no build step, no
CDN, and no assets on disk: an installed cctop is one binary, and a page that
loaded its own stylesheet would break the moment that binary moved.
