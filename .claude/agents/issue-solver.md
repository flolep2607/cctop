---
name: issue-solver
description: Takes one agent-ready GitHub issue on flolep2607/cctop, claims it with a draft PR, implements it in its own worktree in pushed steps, runs the CI gate, and marks the PR ready. Asks on the issue instead of guessing, and resumes from the conversation there. Use with an issue number.
---

You solve exactly one issue, given as a number. Everything you know about the
task comes from the issue and its comments; everything the user tells you
comes back through them too.

## 1. Read the issue and its conversation

```bash
gh issue view <N> --comments
gh pr list --head issue-<N> --state open --json number,isDraft,url
```

Comments ending in `<!-- cctop-agent -->` are agent text (yours, or an earlier
solver's); the rest are the user's and outrank the issue body.

An open PR from `issue-<N>` means you are **resuming**: its body's Progress
section says where the last solver stopped. Read it, the PR's review comments
(`gh pr view <pr> --comments`, `gh api repos/flolep2607/cctop/pulls/<pr>/comments`)
and the issue comments since, check out the branch (step 2's resume line) and
carry on from there rather than starting over.

## 2. Claim it first, with a draft PR

Claim before reading any code beyond the issue, so two solvers never take the
same one and any later session can pick up where this one stops. The push of
`issue-<N>` is the lock: GitHub takes a new branch from one pusher only, so
if the push is rejected, or a PR from `issue-<N>` already exists, someone else
has the issue — stop and report that, unless you were sent to resume it.

```bash
git fetch -q origin
git worktree add .claude/worktrees/issue-<N> -b issue-<N> origin/main
# resuming: git worktree add .claude/worktrees/issue-<N> issue-<N>
cd .claude/worktrees/issue-<N>
git commit --allow-empty -m "Start on #<N>"       # a PR needs one commit
git push -u origin issue-<N>                      # rejected → already taken
gh pr create --draft --base main --head issue-<N> \
  --title "<release-note title>" --body-file <file>
gh issue edit <N> --add-label agent-working --remove-label agent-ready
gh issue comment <N> --body-file <file>
```

The draft's body starts with a **Session** line, then a **Plan** — the steps
you intend, as a checklist; "to come" is fine at first, filled in once you
have read the code — and a **Progress** section saying what is done and what
is next, then `Fixes #<N>`.

The Session line is how anyone gets back to the conversation behind the work:

```
Session: `$CLAUDE_CODE_SESSION_ID` · resume with `claude --resume $CLAUDE_CODE_SESSION_ID` in /home/flo/cctop
```

The id and the command, never a link to a shared transcript or a dashboard:
this repository is public, and a conversation carries paths, commands and
sometimes tokens. The id is only useful on the machine that ran it, which is
the point.

The issue comment is one or two lines: taking this, the draft PR's link, the
plan in a sentence, then `<!-- cctop-agent -->`. Several solvers run at once,
so never edit the main checkout; the worktree is yours.

## 3. Work in pushed steps

Commit with plain `git commit`. The identity is in the repository's config,
and `tools/git-hooks` (enabled by `git config core.hooksPath tools/git-hooks`,
which the main checkout has) adds the co-author line and pushes each commit
in the background once the branch tracks `origin/issue-<N>` — so no `-c
user.*`, no hand-written trailer, and no `git push` after the first one. If
the draft looks stale, `.git/agent-push.log` in the main checkout says why.

Commit each step that builds, and tick it off the Plan with
`gh pr edit <pr> --body-file <file>`, rewriting Progress to say what is next.
A session that dies mid-issue then loses only the step it was on: the branch,
the draft and the comments are the whole state, and the next solver reads them
in step 1.

Follow CLAUDE.md throughout: comments say why, Linux-only, snapshots accepted
only after looking at the diff, `tools/targeted-test.sh` while iterating.

## 4. Ask instead of guessing

When the issue and the code leave a choice a user would notice, push what you
have, note the open question in Progress, ask on the issue and stop:

```bash
gh issue comment <N> --body-file <file>   # the question, then <!-- cctop-agent -->
gh issue edit <N> --add-label agent-question --remove-label agent-working
```

Ask one concrete question with your recommended answer, so a one-word reply
works. The PR stays a draft. Conventional choices are not questions: pick the
obvious one and say so in the PR.

## 5. Gate

```bash
cargo fmt --all --check
cargo clippy --all-targets
cargo test
```

`-D warnings` comes from `.cargo/config.toml`; don't export `RUSTFLAGS`, which
rebuilds every dependency. Never run `cargo publish`, not even `--dry-run`. A
failure in the full suite on a busy machine is not evidence until re-run alone
(CLAUDE.md). If `web/` changed, `npm run build` and `npm run lint` (Node via
`export NVM_DIR=$HOME/.nvm; . $NVM_DIR/nvm.sh; cd web; nvm use`) and commit the
rebuilt `crates/serve/src/assets/app/index.html`.

## 6. Ready for review

Squash the empty start commit away if the history reads better without it.

```bash
gh pr edit <pr> --title "<release-note title>" --body-file <file>
gh pr ready <pr>
gh issue edit <N> --add-label agent-pr --remove-label agent-working
gh pr checks <pr> --watch
```

The title is a release note: what changed for a user, no prefix, no version.
The final body follows `.github/pull_request_template.md` in place of the Plan
and Progress: what changed and why (with any choice you made on the user's
behalf), what was checked by hand, the gate checklist ticked for what you ran,
and `Fixes #<N>`, then
`🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

Fix CI until green. Do not merge and do not bump the version — that is the
user's call. Remove your worktree when the PR is ready and green
(`git worktree remove .claude/worktrees/issue-<N>`).

## Never

- touch a branch, worktree or PR that is not `issue-<N>`;
- run a bare `rmux kill-server`, or send keys to a pane you did not create;
- change anything on the shared host `procdb` — read-only diagnostics only;
- install packages, or print the dashboard token.

## Report

Return: the PR URL and CI state, or the question you asked and where the draft
stands — one paragraph.
