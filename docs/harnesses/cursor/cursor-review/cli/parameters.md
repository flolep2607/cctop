# Parameters

This reference documents every command and flag available in the Cursor Review CLI (`gt`).

The CLI fully supports multiple Git worktrees. Starting in `gt` version `1.8.4`, `gt` does not modify branches checked out in another worktree in most cases.

Each command lists what it does and how to run it. Commands with flags have a quick-reference row below and a collapsible flag table. For the latest details, run `gt COMMAND --help` (replace `COMMAND` with the command name).

## Global flags

Global flags can be used with any command:

| Option          | Description                                                                                                         |
| --------------- | ------------------------------------------------------------------------------------------------------------------- |
| `--help`        | Show help for a command.                                                                                            |
| `--allCommands` | Not printed with global help. Pass `gt --help --all` to print the full list of command help.                        |
| `--cwd <path>`  | Working directory in which to perform operations.                                                                   |
| `--debug`       | Write debug output to the terminal.                                                                                 |
| `--interactive` | Enable interactive features like prompts, pagers, and editors. Enabled by default. Disable with `--no-interactive`. |
| `--verify`      | Enable git hooks. Enabled by default. Disable with `--no-verify`.                                                   |
| `--quiet`       | Minimize output to the terminal. Implies `--no-interactive`.                                                        |

## Commands without flags

Commands that take no flags. Arguments are wrapped in square brackets.

| Command                | Description                                                                | Usage                     |
| ---------------------- | -------------------------------------------------------------------------- | ------------------------- |
| `add [args..]`         | `git add` passthrough.                                                     | `gt add [args..]`         |
| `bottom`               | Switch to the branch closest to trunk in the current stack.                | `gt bottom`               |
| `changelog`            | Show the CLI changelog.                                                    | `gt changelog`            |
| `cherry-pick [args..]` | `git cherry-pick` passthrough.                                             | `gt cherry-pick [args..]` |
| `children`             | Show the children of the current branch.                                   | `gt children`             |
| `completion`           | Set up `bash` or `zsh` tab completion.                                     | `gt completion`           |
| `config`               | Configure the CLI.                                                         | `gt config`               |
| `dash`                 | Open your Cursor Review dashboard.                                         | `gt dash`                 |
| `demo [demoName]`      | Run interactive demos to learn the CLI workflow.                           | `gt demo [demoName]`      |
| `docs`                 | Show the CLI docs.                                                         | `gt docs`                 |
| `fish`                 | Set up `fish` tab completion.                                              | `gt fish`                 |
| `freeze [branch]`      | Freeze a branch and downstack branches to block local modifications.       | `gt freeze [branch]`      |
| `guide [title]`        | Read extended guides on using the `gt` program.                            | `gt guide [title]`        |
| `parent`               | Show the parent of the current branch.                                     | `gt parent`               |
| `pop`                  | Delete the current branch but keep the state of files in the working tree. | `gt pop`                  |
| `rebase [args..]`      | `git rebase` passthrough.                                                  | `gt rebase [args..]`      |
| `reset [args..]`       | `git reset` passthrough.                                                   | `gt reset [args..]`       |
| `restore [args..]`     | `git restore` passthrough.                                                 | `gt restore [args..]`     |
| `top`                  | Switch to the tip branch of the current stack.                             | `gt top`                  |
| `unfreeze [branch]`    | Unfreeze a branch and upstack branches so local modifications work again.  | `gt unfreeze [branch]`    |
| `unlink [branch]`      | Unlink the PR currently associated with the branch.                        | `gt unlink [branch]`      |
| `upgrade`              | Update the CLI to the latest stable version.                               | `gt upgrade`              |

## Commands with flags

Overview of commands that accept flags. Expand a command in the list below for flag details.

| Command    | Description                                                                                   | Usage                   |
| ---------- | --------------------------------------------------------------------------------------------- | ----------------------- |
| `abort`    | Abort the current Graphite command halted by a rebase conflict.                               | `gt abort`              |
| `absorb`   | Amend staged changes to relevant commits in the current stack, then restack upstack branches. | `gt absorb`             |
| `aliases`  | Edit your command aliases.                                                                    | `gt aliases`            |
| `auth`     | Add your auth token so the CLI can create and update PRs on GitHub.                           | `gt auth`               |
| `checkout` | Switch to a branch, or open an interactive selector if omitted.                               | `gt checkout [branch]`  |
| `continue` | Continue the most recent Graphite command halted by a rebase conflict.                        | `gt continue`           |
| `create`   | Create a branch stacked on the current branch and commit staged changes.                      | `gt create [name]`      |
| `delete`   | Delete a branch and CLI metadata; children restack onto the parent.                           | `gt delete [name]`      |
| `down`     | Switch to the parent of the current branch.                                                   | `gt down [steps]`       |
| `feedback` | Send feedback to the CLI maintainers.                                                         | `gt feedback [message]` |
| `fold`     | Fold a branch into its parent, update descendants, and restack.                               | `gt fold`               |
| `get`      | Sync branches from trunk to the given branch or PR from remote.                               | `gt get [branch]`       |
| `info`     | Display information about a branch.                                                           | `gt info [branch]`      |
| `init`     | Initialize the CLI in this repo by selecting a trunk branch.                                  | `gt init`               |
| `log`      | Log stacks (`gt log`, `gt log short`, `gt log long`).                                         | `gt log [command]`      |
| `merge`    | Merge PRs for all branches from trunk to the current branch.                                  | `gt merge`              |
| `modify`   | Amend the current commit or create a new commit; restacks descendants.                        | `gt modify`             |
| `move`     | Rebase the current branch onto a target and restack descendants.                              | `gt move`               |
| `pr`       | Open the pull request page for a branch or PR number.                                         | `gt pr [branch]`        |
| `rename`   | Rename a branch and update metadata (breaks PR branch association).                           | `gt rename [name]`      |
| `reorder`  | Reorder branches between trunk and current; restack descendants.                              | `gt reorder`            |
| `restack`  | Rebase each branch onto its parent so history includes the parent.                            | `gt restack`            |
| `revert`   | Create a branch that reverts a trunk commit (experimental).                                   | `gt revert [sha]`       |
| `split`    | Split the current branch into multiple branches.                                              | `gt split`              |
| `squash`   | Squash all commits on the current branch and restack upstack.                                 | `gt squash`             |
| `submit`   | Push the current stack to GitHub and create or update PRs.                                    | `gt submit`             |
| `sync`     | Sync with remote, clean up merged PR branches, and restack.                                   | `gt sync`               |
| `track`    | Start tracking a branch by selecting its parent.                                              | `gt track [branch]`     |
| `trunk`    | Show the trunk of the current branch.                                                         | `gt trunk`              |
| `undo`     | Undo the most recent CLI mutations from this worktree.                                        | `gt undo`               |
| `untrack`  | Stop tracking a branch (and its children).                                                    | `gt untrack [branch]`   |
| `up`       | Switch to the child of the current branch.                                                    | `gt up [steps]`         |

## Flag reference

### gt abort

| Flag          | Description                                        |
| ------------- | -------------------------------------------------- |
| `-f, --force` | Do not prompt for confirmation; abort immediately. |

### gt absorb

| Flag            | Description                                                    |
| --------------- | -------------------------------------------------------------- |
| `-a, --all`     | Stage unstaged changes before absorbing (not untracked files). |
| `-d, --dry-run` | Print target commits without applying.                         |
| `-f, --force`   | Apply hunks without confirmation.                              |
| `-p, --patch`   | Pick hunks to stage before absorbing.                          |

### gt aliases

| Flag       | Description                                  |
| ---------- | -------------------------------------------- |
| `--legacy` | Append legacy aliases to your configuration. |
| `--reset`  | Reset your alias configuration.              |

### gt auth

| Flag          | Description                                 |
| ------------- | ------------------------------------------- |
| `-t, --token` | Auth token from Cursor Review CLI settings. |

### gt checkout \[branch]

| Flag                   | Description                                 |
| ---------------------- | ------------------------------------------- |
| `-a, --all`            | Show branches across all configured trunks. |
| `-u, --show-untracked` | Include untracked branches.                 |
| `-s, --stack`          | Limit selection to the current stack.       |
| `-t, --trunk`          | Checkout the current trunk.                 |

### gt continue

| Flag        | Description                          |
| ----------- | ------------------------------------ |
| `-a, --all` | Stage all changes before continuing. |

### gt create \[name]

| Flag            | Description                                  |
| --------------- | -------------------------------------------- |
| `--ai`          | AI-generate branch name and commit message.  |
| `-a, --all`     | Stage all changes including untracked files. |
| `-i, --insert`  | Insert between current branch and its child. |
| `-m, --message` | Commit message.                              |
| `--no-ai`       | Disable AI generation (overrides `--ai`).    |
| `-p, --patch`   | Pick hunks to stage.                         |
| `-u, --update`  | Stage updates to tracked files.              |
| `-v, --verbose` | Show commit diff in the message template.    |
| `-o, --onto`    | Create on top of another branch.             |

### gt delete \[name]

| Flag          | Description                          |
| ------------- | ------------------------------------ |
| `-c, --close` | Close associated PRs on GitHub.      |
| `--downstack` | Also delete ancestors.               |
| `-f, --force` | Delete even if not merged or closed. |
| `--upstack`   | Also delete children.                |

### gt down \[steps]

| Flag          | Description                   |
| ------------- | ----------------------------- |
| `-n, --steps` | Levels to traverse downstack. |

### gt feedback \[message]

| Flag                       | Description                          |
| -------------------------- | ------------------------------------ |
| `-d, --with-debug-context` | Include logs from the past 24 hours. |

### gt fold

| Flag          | Description                            |
| ------------- | -------------------------------------- |
| `-c, --close` | Close associated PRs on GitHub.        |
| `-k, --keep`  | Keep the current branch name.          |
| `--stack`     | Fold the entire stack into one branch. |

### gt get \[branch]

| Flag                   | Description                                                         |
| ---------------------- | ------------------------------------------------------------------- |
| `--checkout`           | Check out target after sync (default; use `--no-checkout` to skip). |
| `--delete-all`         | Delete merged or closed branches without prompting.                 |
| `-d, --downstack`      | Do not sync upstack when branch exists locally.                     |
| `-f, --force`          | Overwrite fetched branches with remote.                             |
| `--restack`            | Restack without conflicts (default; use `--no-restack` to skip).    |
| `-U, --unfrozen`       | Checkout new branches as unfrozen.                                  |
| `-u, --remote-upstack` | Include upstack PRs from remote.                                    |

### gt info \[branch]

| Flag          | Description                                 |
| ------------- | ------------------------------------------- |
| `-b, --body`  | Show the PR body.                           |
| `-d, --diff`  | Diff vs parent (overrides `--patch`).       |
| `-p, --patch` | Changes per commit.                         |
| `-s, --stat`  | Diffstat (implies `--diff` if neither set). |

### gt init

| Flag      | Description                             |
| --------- | --------------------------------------- |
| `--reset` | Untrack all branches.                   |
| `--trunk` | Trunk branch name (prompts if omitted). |

### gt log \[command]

| Flag                   | Description                                        |
| ---------------------- | -------------------------------------------------- |
| `-a, --all`            | Branches across all trunks.                        |
| `--classic`            | Old short logging style.                           |
| `-r, --reverse`        | Print upside down.                                 |
| `-u, --show-untracked` | Include untracked branches.                        |
| `-s, --stack`          | Current stack only.                                |
| `-n, --steps`          | Limit upstack/downstack depth (implies `--stack`). |

### gt merge

| Flag            | Description                                  |
| --------------- | -------------------------------------------- |
| `-c, --confirm` | Confirm before merging.                      |
| `--dry-run`     | Report PRs that would merge without merging. |

### gt modify

| Flag                   | Description                               |
| ---------------------- | ----------------------------------------- |
| `-a, --all`            | Stage all changes.                        |
| `-c, --commit`         | New commit instead of amend.              |
| `-e, --edit`           | Edit commit message.                      |
| `--interactive-rebase` | Start interactive rebase.                 |
| `--into`               | Modify a downstack branch.                |
| `-m, --message`        | Commit message.                           |
| `-p, --patch`          | Pick hunks.                               |
| `--reset-author`       | Set author to current user when amending. |
| `-u, --update`         | Stage tracked file updates.               |
| `-v, --verbose`        | Show commit diff in template.             |

### gt move

| Flag           | Description                          |
| -------------- | ------------------------------------ |
| `-a, --all`    | All trunks in interactive selection. |
| `--only`       | Move only this branch.               |
| `-o, --onto`   | Target branch.                       |
| `-s, --source` | Branch to move (default: current).   |

### gt pr \[branch]

| Flag      | Description          |
| --------- | -------------------- |
| `--stack` | Open the stack page. |

### gt rename \[name]

| Flag          | Description                             |
| ------------- | --------------------------------------- |
| `-f, --force` | Rename a branch with an open GitHub PR. |

### gt reorder

| Flag      | Description                                |
| --------- | ------------------------------------------ |
| `--stack` | Include the full upstack through `gt top`. |

### gt restack

| Flag              | Description                            |
| ----------------- | -------------------------------------- |
| `--branch`        | Branch to run from (default: current). |
| `-d, --downstack` | This branch and ancestors only.        |
| `-o, --only`      | This branch only.                      |
| `-u, --upstack`   | This branch and descendants only.      |

### gt revert \[sha]

| Flag         | Description              |
| ------------ | ------------------------ |
| `-e, --edit` | Edit the commit message. |

### gt split

| Flag                        | Description                     |
| --------------------------- | ------------------------------- |
| `-c, --commit, --by-commit` | Split by commit.                |
| `-f, --file, --by-file`     | Split by pathspec (repeatable). |
| `-h, --hunk, --by-hunk`     | Split by hunk.                  |

### gt squash

| Flag            | Description                        |
| --------------- | ---------------------------------- |
| `--edit`        | Edit the commit message.           |
| `-m, --message` | New message.                       |
| `-n, --no-edit` | Keep message (overrides `--edit`). |

### gt submit

| Flag                         | Description                                      |
| ---------------------------- | ------------------------------------------------ |
| `--ai`                       | AI-generate title and description for new PRs.   |
| `--always`                   | Push even if unchanged.                          |
| `--branch`                   | Run from this branch.                            |
| `--cli`                      | Edit PR metadata in the CLI.                     |
| `--comment`                  | Add a PR comment.                                |
| `-c, --confirm`              | Confirm before push.                             |
| `-d, --draft`                | Create new PRs as drafts.                        |
| `--dry-run`                  | Report only.                                     |
| `-e, --edit`                 | Edit metadata for all PRs.                       |
| `--edit-description`         | Prompt for description.                          |
| `--edit-title`               | Prompt for title.                                |
| `-f, --force`                | Force push.                                      |
| `--ignore-out-of-sync-trunk` | Submit despite trunk drift.                      |
| `-m, --merge-when-ready`     | Mark merge when ready.                           |
| `--no-ai`                    | Skip AI.                                         |
| `-n, --no-edit`              | Skip inline edits.                               |
| `--no-edit-description`      | Skip description prompt.                         |
| `--no-edit-title`            | Skip title prompt.                               |
| `-p, --publish`              | Publish submitted PRs.                           |
| `--rerequest-review`         | Rerequest review.                                |
| `--restack`                  | Restack before submit.                           |
| `-r, --reviewers`            | Set reviewers.                                   |
| `-s, --stack`                | Submit descendants (use `--no-stack` to narrow). |
| `--target-trunk`             | Remote trunk for new PRs.                        |
| `-t, --team-reviewers`       | Team reviewer slugs.                             |
| `-u, --update-only`          | Update existing PRs only.                        |
| `-v, --view`                 | Open PR in browser.                              |
| `-w, --web`                  | Edit metadata on web.                            |

### gt sync

| Flag               | Description                                                  |
| ------------------ | ------------------------------------------------------------ |
| `-a, --all`        | Sync across all trunks.                                      |
| `-d, --delete-all` | Delete merged/closed branches without prompting.             |
| `-f, --force`      | Skip confirmation prompts.                                   |
| `--restack`        | Restack when possible (default; use `--no-restack` to skip). |

### gt track \[branch]

| Flag           | Description                            |
| -------------- | -------------------------------------- |
| `-f, --force`  | Set parent to latest tracked ancestor. |
| `-p, --parent` | Explicit parent (tracked branch).      |

### gt trunk

| Flag        | Description                 |
| ----------- | --------------------------- |
| `--add`     | Add another trunk.          |
| `-a, --all` | Show all configured trunks. |

### gt undo

| Flag          | Description                |
| ------------- | -------------------------- |
| `-f, --force` | Undo without confirmation. |

### gt untrack \[branch]

| Flag          | Description                                       |
| ------------- | ------------------------------------------------- |
| `-f, --force` | Untrack without confirmation when children exist. |

### gt up \[steps]

| Flag          | Description                      |
| ------------- | -------------------------------- |
| `-n, --steps` | Levels to traverse upstack.      |
| `--to`        | Navigate toward a target branch. |


---

## Sitemap

[Overview of all docs pages](/llms.txt)
