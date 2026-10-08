---
name: issues
description: Runs cctop's issue loop — files new GitHub issues from one-line requests with the issue-writer agent, and hands agent-ready issues, answered questions and PR review comments to issue-solver agents. Use for "file an issue for…", "work the issues", or under /loop to keep the queue moving.
---

GitHub issues are the queue and the conversation. The user files or comments
from anywhere — the phone included — and this loop turns that into work. Agent
text ends with the marker `<!-- cctop-agent -->`; everything else on an issue is
the user's, because every agent posts under the user's own account.

## Labels

| label | meaning | who moves it on |
|---|---|---|
| `agent-ready` | written and waiting for a solver | this loop |
| `agent-working` | a solver has it, with a draft PR from `issue-<N>` | the solver |
| `agent-question` | the solver asked on the issue and stopped | the user's reply |
| `agent-pr` | the PR is ready for review, CI watched | the user: merge, or review comments |

## With arguments: file issues

`/issues <request>` — spawn one `issue-writer` agent per request (in parallel
for several) and report the URLs. If a writer returns a question, ask the user
it rather than filing.

## Without arguments: one pass over the queue

```bash
gh issue list --state open --label agent-ready    --json number,title
gh issue list --state open --label agent-question --json number,title
gh issue list --state open --label agent-pr       --json number,title
gh issue list --state open --label agent-working  --json number,title
```

1. **agent-ready** — spawn a background `issue-solver` with the number, up to
   three at once in total; more wait for the next pass. Issues that touch the
   same code go to **one** solver together (`Solve #148 and #149`): one PR
   instead of two that conflict, or one waiting on the other. Group by the
   files the issues name, not by label. An issue that says it waits for
   another can instead join that issue's solver while it is still running —
   send it the number.
2. **agent-question** — look at the last comment (`gh issue view <N> --json
   comments --jq '.comments[-1].body'`). Without the marker, the user has
   answered: relabel `agent-ready` and spawn a solver; it resumes from the
   branch and the conversation. With it, the question is still open — skip.
3. **agent-pr** — find the PR (`gh pr list --search "Fixes #<N>" --state all`).
   Merged: nothing to do; GitHub closed the issue. Open with review comments or
   issue comments newer than the solver's last marked one, or failing CI: spawn
   a solver to address them. Otherwise skip.
4. **agent-working** with no solver from this session running on it — a
   solver that died, or one from a session that ended. Its draft PR
   (`gh pr list --head issue-<N> --json number,updatedAt`) is the state: if it
   has not moved for an hour, spawn a solver; it resumes from the draft's
   Progress section. Issues labelled by hand for work outside this loop — a
   branch not named `issue-<N>`, so no draft — are someone else's: skip them.

Never spawn a second solver for an issue whose solver from this session is
still running. Report the pass in a few lines: what started, what is waiting on
the user, which PRs are ready to merge.

Merging and releasing stay with the user.

## Under /loop

`/loop /issues` paces itself; between passes the solvers run in the background
and their completion re-invokes the loop. When nothing is running and nothing
is queued, wake in about 20 minutes to look for new issues and replies —
soon enough for an issue filed from a phone, without polling GitHub for a
queue that is usually empty. CONTRIBUTING.md, "The issue loop", gives the same
figure to the person starting it.
