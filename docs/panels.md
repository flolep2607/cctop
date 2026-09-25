# The bottom panels

[← back to the README](../README.md)

The panel under the table describes whichever session the cursor is on. `←` and
`→` move between panels, `1`–`9` jump to one, `Tab` cycles through them, and
`Shift+↑`/`↓` scrolls inside the active one.

Three of them repay a closer look, and so does the conversation view `i` opens.

## Tool Activity

Each invocation shows the time, its arguments, and — where the transcript
supports it — what it did:

```
19:16 main    ~/cctop/src/ui/render.rs     +43 -24   122ms ↓498.5K ↑ 1.2K
19:34 ↳aa1b82 ~/cctop/src/quota.rs          +2 -0    88ms ↓ 41.2K ↑  310
19:41✗main    cargo test --all-targets               1.4s  ↓ 12.0K ↑  180
```

- **origin** — `main` for the session itself, or `↳<agent-id>` for a subagent.
  Subagent activity is interleaved into the same log, so without this there's no
  way to tell an agent's edits from the parent's.
- **`✗` and a red row** — the call reported an error. Claude records this per
  call, OpenCode and Gemini record a tool status, and Codex is read from the
  sandbox's own result line and exit code. Cursor and Windsurf transcripts don't
  record tool outcomes, so their calls are never marked.
- **`+N -M`** — lines added and removed, from the edit result's patch.
  Press `v` to expand the diff inline beneath the row.
- **duration** — wall time from the call being issued to its result arriving.
- **`↓` / `↑`** — tokens in and out for the assistant turn that issued the call.
  Claude only; Codex transcripts don't tie token counts to individual calls.

That last one deserves a caveat: **billing is per API request, not per tool
call.** When one turn issues several calls they all show that turn's figures,
marked with a leading `*`. Dividing the total between them would invent
precision the transcript doesn't contain. `↓` includes cache reads, which is why
it tracks total context size rather than the size of any one call.

Codex tools are decoded too: `apply_patch` shows the files it touched and its
line counts, `update_plan` shows progress and the step in flight, and
`write_stdin` distinguishes a real write from a poll for more output.

## Reading the conversation

The panels summarise what a session did; `i` shows what it said. It opens an
overlay on the selected row's transcript — a remote row's too, read over the
same ssh link — at the end, so the agent's last reply is the first thing on
screen.

- **Replies are rendered as markdown**: headings, bold and italic, inline code,
  fenced blocks behind a gutter with their language named, lists, quotes,
  tables and rules. Links are OSC 8 hyperlinks, so a click opens them even when
  they wrap; on a terminal that cannot do that (`TERM=dumb` or `linux`) the URL
  is printed after its label. What you typed is shown as you typed it.
- **Tool output keeps its colours.** cargo's green, pytest's red and git's diff
  colours come through, folded to sixteen colours on a terminal that has only
  those and dropped under `NO_COLOR`. Anything else a program wrote — cursor
  moves, line erases, window titles — is removed, so it cannot scribble over the
  view. Expanded Tool Activity rows get the same treatment.

| Key | In the conversation |
|---|---|
| `↑` `↓` `PgUp` `PgDn` `Home` `End` | Scroll |
| `[` / `]` | Previous / next turn, header at the top |
| `m` | Toggle between rendered markdown and its source |
| `u` | Load earlier turns, when there are any |
| `Esc` | Close |

## Context breakdown

![The Context panel: a stacked bar of the window with a legend naming each category](assets/context.png)

`CTX%` says the window is 68% full. The **Context** panel says what is in it —
Claude sessions only, since no other provider's transcript reports per-request
usage.

```
Window   181.4K of 200K

Unaccounted    ━━━━━━━━━━━━━━━━────────────────────────   62.1K  34%
Tool output    ━━━━━━━━━━━━────────────────────────────   43.5K  24%
Startup        ━━━━━━━━━───────────────────────────────   34.5K  19%
Tool input     ━━━━━━━━────────────────────────────────   29.0K  16%
Attachments    ━━━━────────────────────────────────────    9.1K   5%
Assistant text ━━──────────────────────────────────────    3.6K   2%
```

Two of those numbers are measured and the rest are estimated, and the panel
never blurs the line:

- **Window** and **Startup** come from the usage figures the API itself
  reported. Startup is the first request of the live segment — everything the
  harness sends before the conversation begins: the system prompt, the tool
  schemas, CLAUDE.md, the skills index, and, after a compaction, the summary. It
  cannot be split further, because the transcript never records what was sent,
  only that it was.
- **Tool output**, **Tool input**, **Attachments**, **Your messages** and
  **Assistant text** are estimated from how many characters the transcript
  holds, at 2.75 characters per token. That constant is fitted rather than
  assumed: across 167 local sessions, the characters a transcript accumulates
  divided by the context growth the API reports over the same span lands there,
  well under the usual prose rule of thumb because this content is mostly code,
  JSON and file paths.
- **Unaccounted** is the remainder, and it is deliberately a bar of its own
  rather than being spread across the categories that happen to be measurable.
  It runs around a third of the window. Most of it is thinking — Claude Code
  writes those blocks with the text stripped and only the signature left, so
  there is nothing to measure — plus the `<system-reminder>` text the harness
  splices into each turn without recording it, plus estimation error.

A compaction resets the whole thing: everything before the summary has left the
window, so counting across one would describe a context that no longer exists.
Subagent turns are excluded too — they run against their own windows, and only
the report a subagent hands back is in the parent's, where it lands in **Tool
output** like any other result.

When the estimate overshoots the window there is no gap to draw, and the panel
says so instead of clamping: it means the harness has dropped context that the
transcript still holds.

Given the height, the bar folds into a **block map**, like a memory map of the
window: the same cells in the same order, filling rows left to right and top to
bottom, a quarter of the panel's rows and up to six. One row of eighty cells
spends 2.5K of a 200K window per cell; four rows spend 625 tokens, which is the
difference between a small category showing and vanishing. The footnote says
what a cell is worth. Every category in the window gets at least one cell
however small its share, so nothing in the legend is missing from the map. On
the map, the free space past the auto-compaction threshold is drawn as `·`
rather than a lone `┊`, because that is a region — room the harness reclaims
before it is ever reached — and one marker cell is lost in a grid. A panel too
short to spare the rows keeps the one-row bar.

Under the bar, **How it filled** charts the window across every request the
session made. The bar answers "what is in there"; the chart answers "how did it
get that full", which is the part that changes what you do next. A window that
climbed evenly is a conversation that grew and will keep growing. One that
stepped is a handful of large tool results, and the same call will do it again.
A sawtooth is a session living on compactions, paying to rebuild its context
over and over. The chart spans the whole session rather than the live segment,
because a compaction is the most interesting thing that can happen to a context
window and it is the only view that can show one.

## Preview

The last panel, `9`, is the selected row's tab, watched from the dashboard: the
agent's screen as it is right now, updating while you look, without leaving the
table to see whether it has finished or is asking you something.

It is a window, not a way in. Nothing typed on the dashboard reaches the agent,
and the pane is not resized to fit the panel — the agent keeps drawing at the
size its tab gave it, and the panel shows as much of that as fits. When the
screen is taller than the panel, the bottom of what the agent has drawn is what
stays, because that is where its prompt and its questions are. The bottom border
names the tab and the key that goes to it, `Alt+2` onwards.

A tab you have switched away from gives up its rmux client, so there is no
screen of cctop's own to show. The panel asks rmux for the screen instead, twice
a second and only while the panel is on show; that is a read, and rmux resizes
nothing for it.

A row with no tab says so in one line: `a` opens a running session's terminal in
one, `R` resumes a stopped session in one, and either then shows here live. A
row from [another machine](integrations.md#more-than-one-machine) has no tab
here to show.
