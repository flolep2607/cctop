# Merge Queue

Merge Queue experience gives your team one place to line up pull requests, keep the default branch green, and land stacks without making every author babysit rebases.

If your team works in stacks, the queue understands stacked pull requests, so it can process them as a unit instead of treating every PR like an unrelated branch.

## What it does

Merge Queue is built for repositories that have some combination of:

- long CI times
- a steady stream of pull requests landing into `main` or `trunk`
- frequent rebases or broken default branch pushes
- stacked pull requests that need to land in order

Instead of merging directly into the default branch, pull requests enter the queue and wait their turn. The queue decides what runs next, what needs CI, and what can safely merge.

## Requirements

Before you turn it on, make sure these are true:

- Team or Enterprise plan
- Cursor GitHub App is installed for the repository
- Repository is set up to let the App push or bypass the right merge restrictions
- Targeting the repository's default branch

A few important limits are worth calling out up front:

- Currently supports one merge queue per repository
- Merge Queue only supports the repository's default branch
- Merge Queue is separate from GitHub Merge Queue

> If your team already uses GitHub Merge Queue or another third-party queue, use external merge queue integration instead of trying to run both systems as if they were the same queue.

## Set up a merge queue

Open **Merge Queue settings** and choose the repository you want to configure. From there, you can choose between two paths:

- **Merge Queue**: the full stack-aware queue, recommended for most teams
- **External merge queue integration**: use on top of GitHub Merge Queue or another label-based queue you already run

For Merge Queue, the main setup pieces are:

- **Merge strategy**: `Squash and merge`, `Rebase and merge`, or `Merge`
- **Timeout**: how long a pull request can sit at the head of the queue before timing out
- **Merge Queue label**: the label your team can use to enqueue pull requests
- **Fast forward merge**: the switch that unlocks the faster CI and stack-aware queue behavior
- **CI settings**: where CI runs and how much parallelism you want

The app strongly nudges teams toward **Squash and merge**, and it marks plain **Merge** as not recommended.

If your repository uses branch protection rules or rulesets, make sure the App has the right permissions. The queue can work without being the only actor allowed to push to the default branch, but the experience gets worse fast when people merge around it.

## Add pull requests to the queue

The most common way to enqueue work is from the pull request page. When Merge Queue is enabled for a repository, the merge flow changes from "merge now" to "add to queue."

If the pull request is part of a stack, the queue modal can enqueue the full downstack set of open PRs, not just the one you happen to be looking at. That matters. It keeps the stack moving together instead of forcing you to hand-merge each layer.

You can also enqueue through labels:

- apply the **Merge Queue label** to add a pull request to the queue
- if your repo has it enabled, apply the **Fast-track label** to move a pull request ahead of non-fast-tracked work

Some repositories also expose **Merge when ready** toggle, which lets auto-merge once the PR meets the requirements.

## Use the merge queue page

The main queue view is the **Merge Queue** page. It is organized into three tabs:

- **Queued** shows what is waiting, what is actively processing, and what is sitting closest to trunk
- **Activity** shows merges, failures, removals, timeouts, and other queue events
- **Commits** shows recent commits that landed on the default branch

The **Queued** tab is the one you'll use most. It shows:

- the PR or stack's position in line
- whether the queue is paused
- whether a PR is fast-tracked
- whether work is rebasing, running CI, merging, or going through failure handling

From the page-level options menu, you can:

- **Pause queue**
- **Unpause queue**
- open **Merge Queue settings**

Pausing the queue stops merges from landing, but it does not stop people from adding new pull requests to the queue. They will wait there until the queue is unpaused.

## Understand the status on a pull request

When a pull request is in the queue, the PR page shows a merge queue banner so the author does not need to keep switching tabs to understand what is happening.

Depending on the state, the banner may tell you:

- the PR is being added to the queue
- the PR is at the front of the queue
- the PR is next in line
- CI is running in a temporary draft PR
- the PR merged through Merge Queue
- the PR was removed because of conflicts, blockers, timeout, unsigned commits, or another failure

When something goes wrong, the banner usually points you to the next useful place:

- back to the merge queue
- to a temporary draft PR used for CI
- to a failure link
- to commit signing settings if the repo requires signed commits

## Fast-track, remove, and recover

If your repository enables it, a single pull request can be queued as a **fast-track**. Fast-tracked PRs skip ahead of non-fast-tracked work and get processed next.

You can also remove work from the queue. If a PR is already actively being processed, removing it can evict the whole stack and may require CI to run again later. In practice, this means "remove" is easy to click but not always cheap.

If the queue removes a pull request because of a conflict or blocker, the usual fix is simple:

1. rebase or restack the PR
2. fix the failing condition
3. enqueue it again

## CI settings and optimizations

With **Fast forward merge** enabled, you can choose where CI should run:

- **Run on every PR**: every PR in the stack passes CI before merge
- **Run on topmost PR of every stack**: every stack passes CI before merge
- **Run on topmost PR of a group of stacks**: batch multiple stacks together

Depending on your repository's configuration, you can also configure:

- **parallel concurrency**
- **batch wait time**
- **failure handling**
- **bisect mode**
- **maximum failure handling attempts**

Failure handling is especially useful in batch mode. When a grouped CI run fails, can identify the offending stack, re-enqueue the safe ones, and keep the queue moving.

Bisect mode is the slower but cheaper version of that flow. It uses fewer CI runs, which can matter if your pipeline is expensive.

## Temporary draft PRs

If you turn on the more aggressive CI optimizations, this may create temporary draft PRs to run CI on grouped or speculative queue states.

That has two user-facing side effects:

- the PR banner may link to a temporary draft PR while CI is running
- after a successful merge queue run, the original PR may appear as **closed** in GitHub instead of **merged**, even though treats it as merged across the product

## External merge queue integration

If your team already uses another merge queue, the experience differs from Cursor Review.

External integration supports two paths:

- **GitHub merge queue**
- **Third-party merge queue** through labels

This path is useful when you want Cursor Review in the workflow without replacing the queue you already run. The tradeoff is that stacked merges are slower and error handling is more manual than with Merge Queue.

### Enqueue PRs for the GitHub merge queue

To merge a PR, click the **Merge when ready** toggle in the top right of the pull request page.

When the PR is the first open PR in the stack and **Merge when ready** is active, Cursor Review adds it to your GitHub merge queue.

Upstack PRs can also have **Merge when ready** toggled. They merge once the downstack PRs land.

When you merge a single PR at a time with the GitHub merge queue, an upstack PR may show files or commits introduced by a downstack PR that already merged. This is expected. After GitHub merges a PR, it deletes the merged branch and auto-retargets dependent PRs to the base branch (for example, `main`) without rebasing first.

To mitigate this, after the downstack PR merges, run `gt sync && gt submit` locally to rebase the upstack PRs' branches.

If upstack PRs have **Merge when ready** enabled in Cursor Review before the downstack PR merges, they rebase automatically once downstack PRs land.

If you are starting fresh, use Merge Queue. If you already have a queue you cannot replace yet, external integration is the bridge.

## Troubleshooting

### Pull requests are bypassing the queue

Check your branch protection rules or rulesets. If other actors can still push directly to the default branch, people can merge around the queue and force restarts.

### A pull request was removed from the queue

The most common reasons are:

- merge conflicts
- failing checks or other merge blockers
- timeout at the head of the queue
- commit signing requirements

Rebase or fix the blocker, then enqueue again.

### The queue is paused but PRs keep showing up

That is expected. A paused queue still accepts new work. It just stops landing merges until the queue is unpaused.

### GitHub Merge Queue is already enabled

Use external merge queue integration if you need to work with GitHub's queue.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
