# Pull Request Page

The Pull Request (PR) page is where you can review a pull request in detail. It brings together the PR summary, code changes, comments, checks, and merge actions in once place so you can move from understanding the change to taking action on it.

## What you can do on the PR page

- See the pull request title, status, and review actions in the header
- Move through related pull requests when your branch is part of a stack
- Read or update the pull request description
- Follow pull request-level discussion and review activity
- Check reviewers, CI status, labels, assignees, and linked tasks
- Jump through changed files, compare versions, and inspect the full diff
- Leave inline comments, suggested edits, or a full review
- Follow a Code Tour to walk through the change in narrative form
- Read Bugbot findings inline and apply suggested fixes with one click
- Open Cursor Agent in the sidebar to explain code, summarize the diff, draft feedback, or commit code updates directly to the branch

## Navigate the page

### Header and overview

The header keeps the main actions close by. Use it to start a review, merge or publish when you have permission, open comments, and launch Cursor Agent. If a deployment preview is attached to the pull request, it appears here as well.

Below the header, the overview area gives you context before you drop into code:

- **Stack** shows related pull requests when you're working in a stack
- **Description** lets you read or update the pull request summary
- **Discussion** or **Conversation** shows pull request-level comments and review activity, depending on your workspace layout
- **Information** shows checks, reviewers, labels, assignees, and linked tasks from tools like Linear or Jira when configured

### Files and diff

The file tree lets you jump straight to any changed file. You can also switch diff versions to compare different commits or ranges, and mark files as viewed as you work through a review.

Depending on your settings, comments can appear inline with the diff or in a floating review panel. Either way, the file tree, thread list, and code view stay in sync so it is easy to move between files and conversations.

### Code Tours

Code Tours turn a pull request into a guided walkthrough of the proposed changes. Instead of jumping between the description and a scattered set of files, you move through the code in a clear sequence with full context in narrative form.

Cursor Review builds the tour by pulling together everything from the PR. The description, conversations, stack, and code itself come together in one structured readout, so explanation and implementation live side by side. The narrative sits alongside the diff, which keeps you in flow as you review.

Use a Code Tour to:

- Get up to speed on a large or unfamiliar change without backtracking
- Follow the author's intended path through the diff, file by file
- Stay oriented when a PR spans many files or sits inside a stack

## Bugbot review

Bugbot reviews every pull request and flags bugs, security issues, and code quality problems. Findings appear as inline comments on the PR page so you can read them next to the code they reference.

On each finding you can:

- Read the explanation and suggested fix
- Click **Fix in Cursor** to open the issue in your local Cursor IDE
- Click **Fix in Web** to open the issue at [cursor.com/agents](https://cursor.com/agents)
- Ask Cursor Agent to apply the fix directly from the PR page

Bugbot runs automatically on each PR update. You can also trigger it manually by commenting `cursor review` or `bugbot run` on the pull request. With Autofix enabled, Bugbot spawns a Cloud Agent that pushes a fix to the existing branch or to a new branch, then comments on the PR with the result.

For setup, rules, effort levels, and Autofix configuration, see [Bugbot](https://cursor.com/docs/bugbot.md).

## Leave feedback

Hover a line number to start a comment. Click a single line to comment on it, or select a range to comment on multiple lines at once.

From the line actions menu, you can:

- add a comment
- suggest a change
- copy the selected code
- copy a deep link to the selected lines
- send the selection to Cursor Agent

When you're ready to finish, use **Review changes** to approve, request changes, or leave a summary comment.

## Use Cursor Agent on a pull request

Open Cursor Agent from the right sidebar when you want help understanding, responding to, or updating a diff. The agent reads the PR diff, CI failures, and reviewer comments, then takes action with full PR context.

### Update code from the PR page

If you're the author, you can ask Cursor Agent to make code changes without leaving the review. Describe the change in plain language and the agent edits the code, runs scripts and tests as needed, and commits its changes directly to the branch. The new commit appears in the PR like any other commit.

Common updates to ask the agent for:

- Apply a suggested fix from a Bugbot finding
- Address a reviewer comment or change request
- Resolve a CI failure
- Refactor a function or rename a symbol across the diff
- Add or update tests for the change

### Ask questions and draft feedback

Both authors and reviewers can use Cursor Agent to read the diff, not change it. Ask Cursor to:

- Explain why a change was made
- Summarize the risky parts of a pull request
- Draft review comments
- Turn a selected code region into an agent prompt
- Run scripts or execute tests to dig deeper into a change

### Continue threads across Cursor and the PR page

You can start an agent thread in Cursor and pick it up on the PR page, or start on the PR page and continue in your IDE, with full history and context preserved. This lets you kick off a Cloud Agent before the PR exists and finish the conversation in review, or start in review and drop into the IDE for deeper local work.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
