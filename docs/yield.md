# `cctop yield` — did the spend ship?

[← back to the README](../README.md)

`optimize` says what a session wasted and `compare` how a model behaved. Neither
can say whether the work was *kept*. A session that edited forty files, passed
its tests and was then thrown away cost exactly as much as one whose work is in
production, and only the repository knows which was which. So `yield` asks it.

```bash
cctop yield                   # every session that edited a file
cctop yield --since 7d        # this week's
cctop yield --provider codex  # one harness — every session still competes for commits
cctop yield --json            # every session with its class and its commits
```

It reads transcripts and runs `git log`. It fetches nothing and writes nothing.

## The classes

| | |
|---|---|
| **shipped** | A commit of its work is on the default branch, and was not reverted |
| **reverted** | Its work was committed and a later commit reverted it |
| **unmerged** | Committed, but only on branches that never reached the default one |
| **uncommitted** | It edited files in a repository and no commit matched it |
| **in progress** | Nothing committed yet, but it was active in the last four hours |
| **not in git** | Every file it edited is outside any repository |
| **unreadable** | It edited files in a repository git could not read |

Spend is shown per class, then per model and per repository as the share of
each row's own spend that landed in each class. A session with no price —
the harness recorded none, or the model is missing from the price table — is
counted in sessions and never in dollars: a `$0.00` would call it free.

A session that worked in two repositories takes the better of its two outcomes
overall, since one shipped repository means the money bought something; the
per-repository table splits it by the files it edited in each and shows what
happened *there*. Per model, each agent's cost goes to its own model, the way
`compare` credits subagents, and every agent shares the session's outcome — a
commit cannot be traced to the subagent that wrote a given line.

Sessions that edited no file by path are left out and counted on their own
line. Most are conversation or exploration; some wrote through the shell
(`sed -i`, a heredoc), which names no file and so cannot be matched to a
commit.

## How a commit finds its session

A clock alone cannot do this. Two sessions open at once in the same repository —
most of the time, on a machine running several agents — are equally close to
every commit either made. cctop knows which files each session edited, so:

1. A session's **window** runs from its first edit in that repository to four
   hours after its last activity. Four hours because the commit is often not
   the agent's: somebody reads the diff over lunch and commits it. Much longer
   and the window starts catching the next session's commits to the same
   central files.
2. The **candidates** for a commit are the sessions whose window contains its
   author date and that edited at least one file it touches. Author date, not
   committer date, because a rebase moves the latter.
3. The candidate that edited **the most of the commit's files** wins. A tie
   goes to the tighter window. No commit is counted for two sessions.

A file in a worktree under `.claude/worktrees/` counts as the main checkout's,
and since worktrees share one object store its branch's commits are already in
the main repository's `git log --all`. Stashes, notes and merge commits are
left out: none of them is somebody's work on a branch, and a merge's own diff
would let one `git merge` claim every file on a long branch.

**The default branch** is origin's (`origin/HEAD`, else `origin/main` or
`origin/master`) together with the local `main` or `master`, whichever exist;
`HEAD` only when there is neither. Both, because cctop never fetches — a merged
pull request read through a stale `origin/main` would otherwise look unmerged —
and because committing straight to the local `main` is a decision to keep the
work.

**A revert** is a later commit whose message says `This reverts commit <sha>`,
which is what `git revert` writes. A revert of that revert puts the work back,
and a revert sitting on a side branch does not undo work on the default one.

## What it gets wrong

It is a heuristic, and the report says so under every table.

- **Squash and rebase merges over-report *unmerged*.** Both leave the session's
  own commits off the default branch and put new ones there. The new commit
  still matches if it was made inside the session's window and shares its
  files — a squash merged the same afternoon usually is — but a pull request
  squashed a week later reads as unmerged.
- **A stale remote under-reports *shipped*.** Nothing is fetched. A pull
  request merged on GitHub and never pulled is unmerged here until somebody
  runs `git fetch`, unless the local `main` already has it.
- **Commits made by hand compete like anyone's.** Somebody editing the same
  files as an agent, inside its window, can hand the agent their commit — the
  candidate list is sessions, and a person is not one.
- **A worktree outside `.claude/worktrees/`** is a repository of its own as
  far as the path walk can tell. Its commits are deduplicated by sha, so none
  is counted twice, but its files never match the main checkout's.
