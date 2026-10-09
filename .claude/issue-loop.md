---
# issue-loop settings for this project. Every key is optional: delete a line
# (or leave it commented) and the default in brackets applies. Keep the keys
# flat, one per line: the git hooks read `base` and `co_author` with grep.

# Branch the solvers start from and open PRs against.
# [the repository's default branch, from `gh repo view`]
# base: main

# Label names, if the project already uses others for the same states.
# label_ready: agent-ready
# label_working: agent-working
# label_question: agent-question
# label_pr: agent-pr

# How many solver agents the /issues loop runs at once. [3]
# max_solvers: 3

# Trailer added to every commit an agent makes (by the prepare-commit-msg
# hook, or by the solver itself when the hooks are not installed). [none]
# co_author: "Co-Authored-By: Claude <noreply@anthropic.com>"

# The gate a solver must pass before marking a PR ready, one command per
# line. [whatever CLAUDE.md / AGENTS.md / CONTRIBUTING.md / CI says]
# gate:
#   - npm run lint
#   - npm test
---

# Notes for the issue agents

Anything below the front matter is read by the issue-writer and issue-solver
agents as extra instructions for this project, after CLAUDE.md / AGENTS.md /
CONTRIBUTING.md. Use it for what those files do not say and an agent would
otherwise guess — for example:

- how to check a change by hand (start the app, take a screenshot, call an
  endpoint);
- generated files that must be rebuilt and committed in the same change;
- commands an agent must never run here (a deploy, a publish, a migration
  against a shared database).

<!--
Example instructions — replace with your own, or delete this comment:

- Always run `npm run lint && npm test` before pushing, even for a docs change.
- Never touch `vendor/`; it is copied in from upstream and overwritten.
- Regenerate `src/api/schema.ts` with `npm run codegen` when an endpoint changes.
- Keep PR titles to one sentence in plain words; they become release notes.
-->
