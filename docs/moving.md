# Moving a session to another machine

`X` on a row moves the session to another machine: the conversation, and the
files it was working on. The agent is resumed there, and only once it is
running there is it stopped here. Start something at work, press `X`, and it
keeps going on the machine at home.

It is a move, not a copy. Two agents writing one conversation is the thing
every step is ordered to rule out.

## What travels

- **The files, as a git snapshot.** Everything in the checkout as it is now:
  committed work, changes not yet committed, and new files not yet added. Files
  git ignores (`.env`, `node_modules`, build output) do not travel. Your
  branch, your index and your stash here are not touched; the snapshot is taken
  through a temporary index.
- **The conversation.** The transcript is copied into the other machine's
  store, its working-directory fields rewritten for where the checkout landed
  there. Paths inside messages are left as they were written.

## Where it lands

The other machine looks for a checkout of the same repository — the same
`origin` — under its home directory:

- **A clean checkout** with nothing running in it: the session's branch is
  checked out there, moved forward to the session's commit, and the work in
  progress written over it, unstaged. A branch that went its own way there is
  left where it is, and the checkout is left detached at the session's commit.
- **A checkout with work of its own**, or something running in it: a worktree
  beside it (`<checkout>-moved`), so nothing of yours there is moved aside.
- **No checkout**: a new clone at `~/<name>`, with the original's `origin`.

The confirmation shows where, and the directory can be edited: an empty
directory to clone into, or another checkout of the same repository.

## What can move

Claude Code CLI sessions and Codex sessions, in a git checkout, between turns.
Refused, with the reason, before anything changes:

- a session mid-turn or holding a question — wait for its turn to end;
- a directory that is not in a git repository, or a repository with no commits;
- Claude's desktop app, OpenCode, Devin, Cursor, Gemini, Pi and Vibe sessions —
  their transcripts are not files a path can be rewritten in, yet;
- another user's session, a sandboxed one, or one already on another machine.

## The order

1. `X` asks the other machine whether its cctop can take the session, whether
   the harness is installed there, and where it would land. Nothing changes.
2. Enter on the confirmation freezes the agent here (it is idle; this only
   makes sure it stays so), snapshots the checkout, and sends the snapshot and
   the transcript.
3. There, the files are placed, the transcript installed, and the agent
   resumed in a tab. Every step is journalled; if any fails, all of it is
   undone and the agent here carries on.
4. Here, the checkout and the transcript are checked once more against the
   snapshot. If either changed, the other machine undoes its copy and nothing
   has moved. Otherwise the agent here is stopped, its tab closed, and the row
   marked as moved — `R` on it says where it went, rather than starting a
   second agent on the conversation.

## How it reaches the other machine

Two ways, and `X` offers both — the `--host`s first, then the siblings, with
one that cannot be reached right now listed with why:

- **Over ssh**, to a machine named with `--host` (or `$CCTOP_HOSTS`): that
  machine's `cctop move …` is run with everything on its stdin.
- **Over your Cloudflare tunnel**, to another cctop on the same account — the
  machines in the HOST column without a `--host`. The bytes go to its
  `cctop serve` in requests of 2 MB each, signed as this machine, and are
  received there by the same code ssh feeds. Nothing needs setting up beyond
  what already lets you see that machine's sessions here.

Both machines need a cctop that knows how to move a session; an older one there
is named in the refusal, with the update to run.

## Safety on the receiving side

- None of the receiving repository's git hooks, nor its `core.fsmonitor`, run
  while the snapshot is placed; objects are checked as they arrive;
  submodules and LFS are not fetched.
- An `origin` is only written into a new clone if it is `https://`, `ssh://`
  or `user@host:path`.
- Every path is derived on the receiving side; a name from the sender that
  would walk out of where it belongs is refused.
- A move carries at most 2 GB, needs 1 GB to spare on the disk there, and one
  arrives at a time. Over the tunnel the bytes are held there until the last
  one is in, so the disk needs room for them twice; an upload its sender gave
  up on stops counting after 30 minutes.
- Over the tunnel, only another cctop on the account can send one: the routes
  refuse every token link, cookie and Access login, a hub never relays them,
  and each request names its body by sha256 in the signed path, so what was
  signed is what is received.
