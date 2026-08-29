# In a browser

```bash
cctop serve
```

```
cctop: serving on http://127.0.0.1:7777/?t=9f3ac1de…
```

Open that link and you get the table cctop draws in the terminal, streamed live,
plus something the terminal has no room for: a per-session report that says where
an afternoon's money went.

It is **read-only**. It starts nothing, stops nothing, resumes nothing and types
at nothing. Driving agents stays in the TUI, where the thing being driven is on
the same machine as the person driving it.

## Why you would want it

The table's headline is *which session is waiting on you*, and that only pays off
while somebody is looking at the terminal. Agents sit amber for twenty minutes
because the person who could answer them is in a meeting. `cctop serve` puts the
same fact on a surface you carry.

Sessions that need a person are pulled out into their own block at the top of the
page, and a **Needs you** view hides everything else. Sessions group by project,
with idle projects collapsed once there are enough of them that a flat list
would be a wall. The **Notify** button asks for browser notification permission and
then fires one the moment a session *crosses into* waiting — not for the ones
already sitting there when you opened the page, which would be a notification
about nothing.

On a phone the views stay on screen — All, Needs you, Running — and age, sort
and notify sit behind **More**, so the filters are not a strip that clips in
half.

On a keyboard: `/` focuses the filter, `j`/`k` (or the arrows) move, `Enter`
opens the report, `Esc` clears filters. Filter, view, sort and age live in the
URL hash, so they survive a trip into a report and back.

A live session's last tool is on the row — "working · Bash" — so the page
answers *what it is doing* without opening the report.

## The report

Click any row, or go straight to `/session/<id>` (any unambiguous prefix of the
id works, so a link is short enough to paste into chat).

The report is built from a full transcript read, which is why it happens for one
session on request rather than for all of them on a timer. A jump nav at the
top skips to Failures, Changes, Context, Cost and Calls; the call log has its
own filter. It answers three questions, in the order they are usually the
answer:

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

**Or bind wider**, and understand what that means:

```bash
cctop serve --bind 0.0.0.0
```

```
cctop: serving on http://0.0.0.0:7777/?t=9f3ac1de…
cctop: this is reachable from your network — anyone who can open that link can
       read every session on this machine.
```

There is no TLS. A loopback socket does not want it, and terminating it for the
LAN case means certificates cctop has no business managing — which is why the
tunnel is the better answer, since it authenticates rather than merely
encrypting.

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
| `--no-token` | Serve with no access token |
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
