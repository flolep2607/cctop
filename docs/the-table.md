# Reading the table

[← back to the README](../README.md)

One row per session, whether it is running now or ended weeks ago. Columns drop
from the right as the terminal narrows, worst-informing first, so the ones that
say *which session this is* survive to the last. Hover any header for what it
means, click one to sort by it, or press `F6` for the sort list.

Hide columns you never read with `$CCTOP_COLUMNS_HIDE`, a comma-separated list
of the keys below — `CCTOP_COLUMNS_HIDE=tok_rate,mem`.

![The session table with the Overview above it and the Info panel below](assets/dashboard.png)

## The columns

| Column | Key | What it says |
|---|---|---|
| (dot) | `status` | Whether the session is running, and what it is doing — see below |
| `LAST` | `active` | Time since the session last did anything |
| `DUR` | `duration` | First to last activity |
| `$` | `cost` | Estimated cost — see [What the cost figures mean](costs.md) |
| `$/1H` | `cost_hour` | Estimated cost in the last 60 minutes, rolling |
| `$/24H` | `cost_today` | Estimated cost since local midnight |
| `CTX%` | `ctx` | Context window used, as a share of the auto-compact threshold. `COMPCT` while one is happening |
| `CPU%` | `cpu` | CPU across the session's process tree |
| `MEM` | `mem` | Resident memory across the session's process tree |
| `TOOLS` | `tools` | Tool invocations |
| `ERR%` | `errors` | Share of those calls that failed — see below |
| `TOKENS` | `tokens` | Input plus output |
| `TOK/m` | `tok_rate` | Token rate, smoothed |
| `MODEL` | `model` | Model in use |
| `HARNESS` | `harness` | The host application — Cursor, a terminal CLI — as distinct from the model. `─` rather than a guess |
| `PERM` | `perm` | How much it asks before acting — see below |
| `!` | `conflict` | Another agent is on the same ground — see below |
| `HOST` | `host` | Which machine, when [reading more than one](integrations.md#more-than-one-machine). Hidden otherwise |
| `USER` | `user` | Whose session it is, when [watching every user](integrations.md#every-user-on-the-machine). Shown only while more than one user's sessions are on the table |
| `BRANCH` | `branch` | Branch checked out in the working directory, `@<commit>` when detached, `─` when not a repository |
| `PROJECT` | `project` | The session's title if it has one, otherwise its working directory |

## The status dot

The left status dot is green while an agent is working, amber after its latest
response is waiting for your input, and red when the newest transcript event is
an API error. A hollow grey dot is a stopped session, and a filled `◉` is the
session that rang in the last 30 seconds.

A live session whose turn ended while you were not looking at it wears a `✓`
in the accent colour instead of its amber dot: *done, unseen*. Amber says the
prompt is yours, which is true of most agents most of the time; the check says
this one has news since you last looked. It is cleared by looking, which means
one of two things — its row is the selected one while the dashboard is on
screen, or its pane is the focused pane of the tab you are on. A split's other
pane does not count, for the same reason it still lights up the tab bar: you
are not reading it. The tab bar carries the same `✓` after the tab's label,
and `Alt+b` goes there once nothing is blocked on a question.

The mark belongs to this cctop alone. A tab's name and colour follow it into
every cctop on the machine, but having read a reply on your laptop says
nothing about the cctop on your phone, so seen-ness is kept in memory and not
written anywhere. It also only marks a turn it watched end: a cctop started
after an agent went quiet has no news to report about it.

A session past one of the `alert_*` thresholds you have set wears that alert
in place of its dot for as long as it stays past it: `$` for its cost or burn
rate, a red `!` for an error loop, `◌` for a working agent that has written
nothing for a while. See [alerts](driving-agents.md#alerts-on-spend-error-loops-and-stalls).

## `PERM` — how much a session asks

**PERM** is how much a session asks before it acts: `ask`, `edits` (writes files
unasked), `plan` (cannot act at all), or a red `BYPASS` for one started with
`--dangerously-skip-permissions`. Read from the transcript, and kept current by
the session's own hooks when it has them. `─` means the harness does not record
it — today only Claude Code does.

## `ERR%` and compaction cadence — sessions that are not getting anywhere

A session can be busy and expensive without getting anywhere, and the columns
that measure how hard it is working all read *higher* when that happens. Two
that read the other way:

**`ERR%`** is the share of a session's tool calls the transcript reported as
failed. A few is ordinary — a grep that found nothing, a build that caught a
mistake. A quarter of them is an agent retrying something that will not work,
paying full price for each attempt. Sort by it with `F6` to put those at the
top; the Info panel gives the two numbers behind the rate, and Tool Activity
marks the individual calls with `✗`.

It reads `─` where the harness records no per-call outcome — Cursor, Pi and
Windsurf — rather than `0%`, which would claim a clean run cctop cannot see.
Claude, Codex, Gemini and OpenCode all report one.

**Compaction cadence** is under the Context panel's chart. The sawtooth in that
chart is already the shape of a session living on compactions, but three of them
over two days is a long conversation while three in twenty minutes is a session
that will spend the rest of the day rebuilding a window it keeps refilling — and
the chart draws those identically. From three compactions on, the panel says how
often: `↺ one compaction every 15m`. Claude Code only, since no other transcript
records that a compaction happened.

## `!` — when two agents are in one repository

Two agents editing one checkout is not a merge conflict. Git would at least
announce that. It is one of them writing a file the other is still holding in
context, and the loser finds out when the work is already gone. cctop is the
only thing on the machine that can see both of them, so it is the only thing
that can say so while it still helps.

The `!` column is that warning:

| | |
|---|---|
| `⚠` | another running agent has written a file this session also wrote |
| `·` | another running agent is in the same repository, and has not touched your files |
| (blank) | nobody else is here |

Sort by it with `F6`, and the Info panel names the peer and lists the files. The
footer carries the `⚠` case only — agents share repositories all day and nothing
has gone wrong yet, whereas two of them writing one file means an edit has
already been lost or is about to be.

Some files are exempt, on one rule: **nothing exempt is work that can be
silently lost.** Being plain text is not the test — source code is plain text,
and losing a line of it is why the warning exists.

| | |
|---|---|
| prose | `.md` `.markdown` `.rst` `.adoc` `.org` `.txt` |
| machine output | `.lock` `.sum` `.log`, plus `package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml` |

Prose is what two agents are *supposed* to be writing at once — a changelog, a
notes file, a memory index — edited by section rather than rewritten whole. A
`⚠` that fires every time both of them append to `MEMORY.md` is one that gets
read past, and it takes the `src/ui.rs` case with it.

Machine output is nobody's handwriting. Two agents in one checkout both running
`cargo add` or `npm install` write a lock file every time, and the fix for a bad
one is to regenerate it; a real disagreement about a dependency surfaces in git,
which announces it properly. npm and pnpm spell their lock files `.json` and
`.yaml`, so those two are named outright — the extensions themselves stay
contested, because two agents editing one `package.json` is exactly the case the
warning is for.

The exemption is per file, not per session: share a notes file and a source file
and the source file is still reported.

The unit of comparison is the **repository root**, not the working directory. A
linked worktree carries its own `.git`, so two agents in two worktrees of one
repository are editing two sets of files on disk and are not reported; two
agents started from different subdirectories of one checkout are. Comparing
directories gets both of those backwards, and the second is the arrangement
`git worktree` exists to provide.

The repository is the unit for the `·` case — the neighbourhood. A shared file
is stronger evidence and is not limited by it: two sessions holding one path
on disk are racing for it wherever they were launched from. An agent working
a parent checkout can write into a nested checkout another agent owns, and an
edit landing outside the repository has no ground to compare at all.

Four limits worth knowing. Two agents rewriting one README whole *can* lose an
edit, and the prose exemption above will not say so. Only running sessions are compared — a session that
has stopped may well have left uncommitted work behind, but nothing it does from
here can race anyone. Only the last 32 files each session wrote are watched, so
a path it finished with an hour and forty edits ago is not treated as contested.
And a Codex `apply_patch` covering several files summarises as
`first.rs (+3 more)` in the transcript, so only the first of them is recovered.

Agents can ask this themselves through `check_conflicts` — see
[Letting agents see each other](integrations.md#letting-agents-see-each-other).

### Telling the second agent

The `!` column warns *you*, and by the time you look the second agent has
usually made its edit. With the hooks installed, cctop can tell that agent
instead, at the moment it reaches for the file. It is off by default; turn it on
in the settings panel (`,`) or in `config.toml`:

```toml
[settings]
warn_agents = true
```

From then on every file write a hook sees is noted in a small ledger beside the
hook sockets — which file, which session, when, and the agent process that made
it. When a session is about to write a file that a *different* agent, still
running, wrote in the last 30 minutes, its hook answers with a note the model
reads:

```
cctop: another agent that is still running on this machine wrote this file recently.
- /home/you/proj/src/ui.rs — 3m12s ago, by Claude Code session 1a2b3c4d working in /home/you/proj
Two agents editing one file do not merge: whichever writes last silently replaces
the other's work. Re-read the file before changing it again, and settle who finishes
first — or move one of you into a separate git worktree.
```

| harness | told on | through |
|---|---|---|
| Claude Code | `PreToolUse` of `Write`, `Edit`, `MultiEdit`, `NotebookEdit` | `additionalContext`, which reaches the model with the tool result |
| Gemini CLI | `AfterTool` of `write_file`, `replace` | `additionalContext` — `BeforeTool` has no way to add context |
| Codex | not told | its `apply_patch` writes are recorded, so the others hear about them |

It is advice and nothing more: the answer carries no permission decision, so it
cannot block, prompt or deny, and the hook still exits 0 inside its 250ms
deadline. Anything that goes wrong on the way — no ledger yet, a mangled one,
another hook holding its lock — is the same silence as the setting being off.

It needs no cctop running: the hooks keep the ledger between themselves. It
makes the same exemption as the `!` column (prose and lock files are never
mentioned), a session is never warned about its own writes, and a writer whose
process has exited no longer counts. Codex is recorded but never answered, and
Cursor neither, because neither documents what it does with a hook's JSON on a
tool call — a guess there is how a monitor becomes an error on every edit.

## Finding a session

`/` filters the table as you type, on everything a row is: its label or title,
the full working directory (not just the abbreviation the column has room for),
the git branch, the model, the harness, the provider and the session id. The
cell that matched is underlined, so it is clear *why* a row survived the filter.
`n` and `N` step through matches, `Esc` clears, and `↑`/`↓` inside the prompt
bring back a search you ran before — the last twenty are remembered across runs.

`user:<name>` narrows to one person's sessions when cctop is
[watching every user](integrations.md#every-user-on-the-machine), without the
name also matching every title that happens to contain it. Part of a name is
enough, the rest of the query still applies (`user:ana flaky test`), and two
`user:` terms show either user's rows.

`Tab` widens the search to the transcripts themselves, which is how you find the
session where something was actually discussed rather than one whose name
happens to mention it. Transcript matches are added to the metadata matches
rather than replacing them, and the line each one was found on is shown under
the prompt.

This reads every transcript on disk, so it is opt-in, it waits for a pause in
typing and for a query of at least three characters, and it runs on cctop's
background thread pool — the table stays live throughout, and the footer says
`+transcripts…` while a scan is out. Results are remembered per query, so
refining a search re-reads only what it must. Two limits are worth knowing:
transcripts store their text as JSON, so a phrase containing a quote or a
newline is escaped on disk and will not match; and a single session is scanned
up to 64 MiB.

## The tree view

`T` groups the table the way htop's tree mode groups processes: each
repository gets a heading, each checkout of it a heading beneath that, and the
sessions hang off the checkout they were started in. When the rows shown belong
to more than one user — root [watching every user](integrations.md#every-user-on-the-machine)
— each user gets a heading above their repositories, so two people in the same
repository are two groups that fold separately.

```
   LAST       $  BRANCH  PROJECT
●    4s  $18.40          ▾ ~/cctop  4 sessions, 1 waiting
●    4s  $11.02  main    ├─ ▾ cctop (main)  2 sessions, 1 waiting
●    4s   $9.10  main    │  ├─ Improve super cctop
○    3h   $1.92  main    │  └─ Release 0.17.8
●   12s   $7.38  tree    └─ ▾ .claude/worktrees/agent-a9c7  2 sessions
●   12s   $5.01  tree       ├─ Tree view for the table
●    1m   $2.37  tree       └─ Search tiers
```

The glyphs are drawn in the PROJECT column, so every other column stays under
its header and sorting by one still works.

A repository is recognised by its git common directory, which every worktree
of it shares — so a linked worktree lands under the repository it was taken
from rather than as a stranger. A repository seen through only one checkout
skips the second level. A directory outside any repository is a group of its
own, and a row from another machine is grouped by host and directory, since
the far filesystem is not there to ask.

A heading carries what adds up: how many sessions are under it and how many of
those are waiting on you, their total cost, the latest activity, and for a
checkout the branch it has out. Its dot is the loudest state beneath it, so a
folded heading still shows red when something inside is asking a question.

`Enter`, `Space` or `e` on a heading folds and unfolds it. They do nothing else
there — a heading has no menu and cannot be marked — while `←`/`→` stay on the
bottom panels on every row, heading or not. Subagents still expand under their
session with `e`, one level further in. Sessions are never nested under each
other: no harness records that one session started another.

Filtering shows the sessions that match with the headings above them, and a
heading's totals count only what is shown. The sort applies inside each group,
and groups are ordered by their best-placed session — a cost sort puts the
repository with the dearest session first — without a group ever being split.
`b` unfolds whatever is hiding the session that rang. The view and the folds
are remembered across runs.

## Idle sessions, and the memory they hold

An agent left open keeps its whole process tree: a `claude` process is a few
hundred megabytes, and an MCP server it started is often another hundred. On a
shared machine that is where the RAM goes — twenty sessions a few days old is
ten gigabytes that nobody is typing into.

`I` narrows the table to those: sessions that still have a process but have
written nothing for `idle_after` hours (6 unless `,` says otherwise). They are
sorted by the memory of their process tree, the root and every child — the
figure the Processes panel adds up — and the MEM column stays on screen however
narrow the terminal. `LAST` is how long each has been quiet. The table's title
says what a stop would give back:

```
╭ Sessions (14/340) — idle ≥6h: 12 sessions, 5.4G reclaimable ──────╮
```

`K` in this view stops them. With rows marked it stops the marked ones; with
none marked it stops every one the view shows, since the view is already the
selection. A confirmation lists each with its idle time and memory, and the
total. Some are left running, and are named with the reason:

- **working** — its own hooks say a turn is under way;
- **asking a question** — a permission prompt or an elicitation is waiting on
  you;
- **busy (N% CPU)** — nothing in the transcript, but the process tree is using
  at least 5% of a core, which is what a long build or a background shell
  looks like from outside;
- **on host** — a row from another machine, which cctop reads and does not
  signal.

Stopping is SIGTERM to the agent, the same as `Ctrl+K`, never SIGKILL: the
agent writes its transcript out and takes its children with it, so `R` resumes
any of them later. What cctop cannot see is a prompt typed into one and not
sent, which goes with it — the confirmation says so.

`I` again, or `Esc`, puts the table back the way it was sorted. When a stop
would give back more than a gigabyte the overview says so beside the agents'
total, as `Agent mem 10035 MB · 5.4G idle (I)`, so the view does not have to be
remembered to be found.

## The row menu

Every action below has a key, and the keys are worth learning. But you have to
know they exist first, and a key that quietly declines — because the row is on
another machine, or is a subagent, or has no process left to signal — teaches
nothing about why.

`Enter` on a row opens everything you can do to it:

```
╭ Improve super cctop ─────────────────────────────────────╮
│ Resume in a tab                                        R │
│ Restart it in its tab    it is not running in a tab here │
│ Attach to it                                           a │
│ Type into it               no local process to type into │
│ Hand off to another agent                              O │
│ Show its subagents                                     e │
│ Mark for a batch action                            space │
│ ──────────────────────────────────────────────────────── │
│ Terminate the agent                    it is not running │
│ Delete the transcript                                  d │
╰ ↑↓ Enter · Esc ──────────────────────────────────────────╯
```

Entries that cannot run stay on the list, greyed, with the reason where their
key would be — one or the other, never both, since pressing the key of a
refused entry only repeats the refusal. They are not hidden: a menu that
changed shape from row to row would teach you less than one that says why.

The cursor never lands on a refusal, so `Enter` always does something. The
letters stay live inside the menu, so `Enter` `R` and a plain `R` are the same
two keystrokes — the menu shows the shortcuts rather than replacing them.
Clicking works too. `Esc`, or a click outside, closes it.

## Every key

| Key | Action |
|-----|--------|
| `Enter` | Everything you can do to this row, in one menu (see above) |
| `↑`, `↓`/`j` | Move between sessions |
| `PgUp`, `PgDn` | Page up / down |
| `Ctrl+U`, `Ctrl+D` | Half a page up / down |
| `g`, `G` | Jump to first / last |
| `Home`, `End` | Jump to first / last |
| `n`, `N` | Next / previous search match (wraps) |
| `w` | Toggle notifications (see below) |
| `W` | Share the agent's terminal to a browser (needs rmux, see [Driving agents](driving-agents.md)) |
| `b` | Jump to the session that rang last |
| `←`, `→` | Move between bottom panels |
| `1`–`7` | Jump to a panel directly (`Tab` also reaches Context, the eighth) |
| `Shift+↑`/`↓` | Scroll inside the active panel |
| `Shift+Home`/`End` | Jump to the top / bottom of that panel |
| `f` | Follow mode: keep the selection centered |
| `/` or `F3` | Filter sessions by text (see below) |
| `S`, `F6`, `>`, `<` | Sort-by panel |
| `F7` | Filter by age (1d / 1w / 1mo) |
| `#` | Cost floor: only sessions costing ≥ `$X` |
| `,` | Settings and keybinds (see below) |
| `` ` `` | Show only running sessions |
| `I` | Idle view: live sessions quiet for `idle_after` hours, most memory first (see above) |
| `[`, `]` | Move through the Tool Activity tool filter |
| `v` | Toggle inline diffs for edits |
| `L` | Toggle the Tool Activity live filter |
| `T` | Tree view: group by repository and worktree (see above) |
| `+`, `-`, `=` | Speed up / slow down / reset refresh interval |
| `Space` | Mark / unmark the selected session |
| `D`, `K` | Delete / terminate all marked sessions (with confirmation); in the idle view `K` stops the idle ones |
| `U` | Clear all marks |
| `h` or `F8` | Agent integration: what reports to cctop, and install it |
| `y` | Copy resume command or transcript path |
| `d` | Delete the selected session (not running) |
| `k` | Terminate the selected live session (with confirmation) |
| `s` | Type a line into the selected session's terminal (see below) |
| `R` | Resume the selected session in a tab of its own (see below) |
| `Ctrl+R` | Restart every agent tab on the same session, skipping any mid-turn |
| `O` | Hand the selected session's context off to a different agent (see below) |
| `a` | Open that session's terminal in a tab and drive it |
| `t` | New tab: run an agent or a shell (see below) |
| `Esc` | Clear the active filter |
| `q` or `F10` | Quit |

Tabs and splits, from anywhere including inside a running agent:

| Key | Action |
|---|---|
| `t` or `Alt+n` | New tab: an agent, a shell, or one still running |
| `Alt+v` / `Alt+s` | Split the current tab right / down |
| `Alt+←` / `Alt+→` | Previous / next tab |
| `Alt+1`–`9` | Jump to a tab; `Alt+1` is the dashboard |
| `Alt+t` | Pick a tab from a list, typing to narrow it |
| `Alt+b` | Jump to the next tab whose agent needs you, then to one whose turn ended unseen (`✓`) |
| `Alt+r` | Rename or recolour the tab you are on |
| `Alt+o` | Move focus to the next pane |
| `Alt+z` | Zoom the focused pane to fill the tab, or put the split back (see below) |
| `Alt+w` | Close the focused pane and stop its agent |
| `Alt+Shift+W` | The same thing, by a name that says so |
| `Alt+Shift+R` | Restart the pane's agent on the same session, after an update; on the dashboard, the selected row's tab |
| `Alt+Shift+C` | Record the focused pane to an asciinema `.cast`; again to stop (see [Recording a pane](driving-agents.md#recording-a-pane)) |
| `F9` | Paste the clipboard's image (see below) |
| `F12` | Back to the dashboard, leaving everything running |

`Alt+z` zooms the focused pane over the whole tab, and the bar marks the tab
`⤢` while it is. The other panes keep running out of sight — their output is
still read, so none of them stalls — but they are not resized: each agent goes
on at the size it had in the split, so zooming costs the hidden ones no
redraw. Only the zoomed agent is resized, once each way. `Alt+o` while zoomed
moves the zoom to the next pane; splitting again unzooms, so a new agent is
never started out of sight. An agent another cctop is also showing is drawn at
the smaller of the two sizes, as always, so zooming cannot grow it past that.

Every function key is cctop's, inside a pane as much as on the dashboard: none
of them is passed to the agent. `F10` (quit) and `F5` (refresh) act where you
press them; `F9` pastes into the pane you are in; `F12` returns to the
dashboard; `F1`, `F3`, `F6`, `F7`, and `F8`
bring the dashboard forward and then do what they do there, since a search box
or a sort order over a pane would be drawn on a screen the agent is repainting.
An unbound function key does nothing rather than reaching the agent as an escape
sequence.

Mouse works too: click session rows, column headers, and panel tabs; scroll
anywhere. In Tool Activity, click any row to expand the full untruncated
argument, and click the sidebar to filter by tool.

### Settings and keybinds

`,` opens every setting and every key on the session table at the value it has
now, a `*` beside the ones you have changed. `Enter` changes the row under the
cursor — a toggle flips, the theme turns to the next one, and a key waits for
you to press the combination it should move to — and `Backspace` puts it back.
A key another action was on is still taken; the status line says which action
lost it. `e` opens the file itself in `$VISUAL` or `$EDITOR`, and an edit saved
there applies at the next keypress.

It all lives in `config.toml` under your config directory, beside any account
tokens, and only what you changed is written:

```toml
[settings]
theme = "light"          # auto / light / dark / mono
notify = true
compact_threshold = 90   # context % the agent compacts at
idle_after = 12          # hours quiet before `I` counts a live session idle

[keys]
quit = "x"
bottom = "shift+down"    # ctrl+, alt+ and shift+ all work
```

Only the session table's keys move. A modal's keys are the letters on its own
buttons, and inside a pane the keyboard is the agent's.

### Pasting an image

A terminal cannot carry one. A bracketed paste is text, so a screenshot copied
with the system's own shortcut reaches an agent as nothing at all — which is
why `Ctrl+V` in a pane appears to do nothing when the clipboard holds a picture.

`F9` is the way in. It reads the image off the clipboard, writes it to
`~/.cache/cctop/pastes/paste-<when>.png`, and types *the path* at the agent,
followed by a space — the form every one of these harnesses already reads an
image in. No image bytes go near the pty. On the dashboard the same key opens
the type-into box with the path in it, so a session you are not attached to can
be sent one too. For a few seconds after either, the image itself sits in the
bottom-right corner — the confirmation is the picture, not only its name.

Inside a pane `Ctrl+V` does it too, but only when the clipboard actually holds
a picture: with text on it the key goes to the agent untouched, as it always
did. Whether it arrives at all is the terminal's decision — Windows Terminal
binds `Ctrl+V` to its own paste and never tells the application, so there `F9`
is the way in, or unbind it in the terminal's settings.

The right button is deliberately not a third way. It was for a day: in Windows
Terminal it *copies* when there is a selection and pastes only when there is
none, so selecting some output and right-clicking to copy it pasted an image
into the agent instead. cctop cannot see the terminal's selection, so it cannot
tell the two apart.

It needs something that can read the clipboard: `wl-clipboard` or `xclip` on
a Linux desktop, and `powershell.exe` under WSL, where the clipboard being read
is Windows'. Under WSL it is also looked for at
`/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe`, because a
session nobody logged into by hand — an ssh into the distribution, a cron job —
has none of the Windows interop entries on its `PATH`, and the same machine
would otherwise report having no way to read its own clipboard. cctop says which of the two things went wrong: an empty clipboard
and a machine with no such tool need different answers.

### Pasting an image into a cctop you sshed to

No helper on the far side can reach it — the clipboard is on the machine you
typed the `ssh` on. So `F9` goes the other way and asks the terminal that is
showing you cctop, over the escapes the connection already carries: kitty's
clipboard protocol can hand back `image/png` itself, and any terminal that
answers the classic OSC 52 read gives up its clipboard as text, which files
the same when the text is a base64 image. Most terminals refuse the read —
the answer to "what is on the clipboard" is something no program should get
unasked — so kitty wants `read-clipboard` in `clipboard_control` before it
will say, and where the terminal stays silent `F9` still says so rather than
telling you to install `xclip`.

For a terminal that will never answer — Windows Terminal does not, and no
Ctrl+V or right-click there can put an image on the wire — the way in is the
connection itself. `ssh` carries sockets back down the link it opened, so a
small listener on the machine you ssh from can hand the clipboard over.
`tools/clipboard-bridge.ps1` is that listener, on Windows:

```powershell
powershell -ExecutionPolicy Bypass -File clipboard-bridge.ps1
```

with the port forwarded — `ssh -R 8377:127.0.0.1:8377 <host>` on the command
line, or `RemoteForward 8377 127.0.0.1:8377` in `~/.ssh/config` to make it
permanent. cctop asks by connecting; the bridge answers with the clipboard's
image, or with a closed connection when there is none. `F9` uses it, and so
does Ctrl+V in a pane — the ask is a localhost connect that costs nothing
where no bridge is listening — so from Windows Terminal an image pastes the
way text does. The port is `CCTOP_CLIPBOARD_PORT` on the far side and `-Port`
on the bridge when 8377 is already taken.

The way that needs nothing of the terminal or a script is the browser. `cctop serve` on
that machine puts the table on an HTTP port; reach it from your own machine —
`ssh -L 7788:127.0.0.1:7788 <host>`, or `--tunnel` — and paste the screenshot
straight into the box that answers a waiting session. A browser can take a
real image off the clipboard where a terminal cannot, so the bytes travel over
the connection cctop already has, land in `pastes/` on the far machine, and
the box fills with the path. Then write the sentence around it and send.

The other way needs no browser. What crosses a terminal is text, so send the
image as text: cctop reads any paste that is an image in base64 — bare, or as
a `data:image/…;base64,…` URI — writes it to the same `pastes/` directory, and
gives the agent the path. The decoded bytes are sniffed for a real format —
PNG, JPEG, GIF, WebP, BMP — so an ordinary paste is never mistaken for one.

Encoding it is a one-liner where your clipboard is. In PowerShell, which is
where you would be if you sshed from Windows:

```powershell
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$i=[Windows.Forms.Clipboard]::GetImage(); $m=New-Object IO.MemoryStream
$i.Save($m,[Drawing.Imaging.ImageFormat]::Png)
Set-Clipboard ([Convert]::ToBase64String($m.ToArray()))
```

That replaces the clipboard's image with its own base64, which then pastes into
the agent as an image. On a Mac or a Linux desktop the same thing is
`pngpaste - | base64 | pbcopy` and `wl-paste -t image/png | base64 -w0 |
wl-copy`.

The images are kept, not cleaned up: the path is in a transcript by then, and a
conversation resumed next week may read it again. The directory is yours to
empty.

Right-click a row for its menu, the same menu `Enter` opens. The footer's key
hints are buttons — clicking `? Help` opens the help, clicking `R Resume`
resumes — and so are the `[y]` and `[n / Esc]` in every confirmation, where a
click beside the dialog is also a cancel. `q Quit` is the one that takes two
clicks: it is the only key on the footer with nothing behind it to ask again,
and the first click says so in the status line.

### More than one account

Claude Code and Codex both let one machine hold several logins — `$CLAUDE_CONFIG_DIR`
for the first, `$CODEX_HOME` for the second — and a second subscription usually
exists because the first one runs out of window before the day does.

cctop finds them the same way for both: the directories beside the conventional
one that are actually signed in, so `~/.claude-work` and `~/.codex-work` are both
an account called `work`. Where there is more than one, the new-tab launcher
carries an `as <name>` line and `p` cycles it — the choice is remembered, because
somebody working out of their work login is working out of it all afternoon.

Everything downstream follows the account rather than assuming the default: rows
from every login appear in the table, the PROFILE column says which one each came
from, the limits panel reports each subscription's own 5h and weekly figures, and
`R` resumes a session under the account whose directory the transcript lives in —
without which a resumed Codex session would come back blank, its id being unknown
to any other home.

An account whose limits cctop cannot read from its directory can be given a
token instead. `claude setup-token` prints a long-lived one, and

```bash
cctop --add-account work   # reads the token from stdin, so it stays out of history
```

keeps it in `config.toml` under your config directory, readable only by you:

```toml
[accounts.work]
token = "sk-ant-oat01-…"
```

The name is the account's. Where a `~/.claude-work` exists it is that profile's
token, winning over the credentials in the directory — being explicit is the
point of typing it in. Where no such directory exists, the token *is* the
account: it gets its own column in the limits panel, and its sessions stay in
the one `~/.claude` with everything else, sharing the history, the settings and
the project trust rather than splitting them across a second directory. Start
one with `CLAUDE_CODE_OAUTH_TOKEN` set and that is the subscription it spends.

Such an account is not offered in the launcher's profile picker, and cctop's
`R` never resumes under it: both work by putting the account in front of the
command, and the only thing there is to put there is the token itself — which
would be in every `ps` on the machine. Set the variable in the shell you launch
from instead.
`CLAUDE_CODE_OAUTH_TOKEN` wins over both, for the default profile only: Claude
Code prefers that variable too, so a session started with it spends the account
the variable names rather than the one the directory holds. One variable is one
value per machine, which is why it answers for one profile rather than being
reported as every account's usage.

In the tab bar, drag a tab to move it along the bar — the arrangement is
written onto the rmux sessions, so it is still there after `F10` and in every
other cctop on the machine — and right-click one, or press `Alt+r` on it, to
name it or paint it: `3:claude-4` says nothing about what that agent is doing,
and the name and colour you give it follow the tab into every cctop on the
machine. `←`/`→` walk the colours in the same prompt; the leading `○` stop is
no colour at all. A painted tab wears its colour in the bar and on the title
of every pane border it owns.
