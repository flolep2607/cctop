# Pull request commands

Origin is currently released in early beta. You can create repos, push and pull with git, mirror from GitHub, browse and search code, open and merge pull requests, and share with your Cursor team.

Please submit any and all feedback to [hi@cursor.com](mailto:hi@cursor.com) to help us make the product better.

Every `origin pr` subcommand with its options. For the rest of the CLI, see [Commands](https://cursor.com/docs/origin/cli/reference/commands.md). For how pull requests work in Origin, see [Pull requests](https://cursor.com/docs/origin/pull-requests.md).

## Targeting a pull request

Pull request commands infer the repository from the `origin` git remote in your current checkout. Override it with `-R, --repo` in `org/name` format.

Most subcommands take an optional `[target]`. A number selects that pull request, anything else is read as a branch name. Omit it and the CLI uses the open or draft pull request for your current branch:

```bash
origin pr view 13        # pull request 13
origin pr view my-branch # pull request for my-branch
origin pr view           # pull request for the current branch
```

## Subcommands

| Subcommand          | Description                                                          | Usage                           |
| ------------------- | -------------------------------------------------------------------- | ------------------------------- |
| `create`            | Open a pull request against an Origin repo                           | `origin pr create -t "Fix CI"`  |
| `list` \| `ls`      | List pull requests in a repository                                   | `origin pr list`                |
| `status`            | Show the pull requests relevant to you in a repository               | `origin pr status`              |
| `view [target]`     | Display a pull request, and optionally its diff, checks, or comments | `origin pr view 13`             |
| `diff [target]`     | View the diff for a pull request                                     | `origin pr diff`                |
| `checks [target]`   | Show CI status for a pull request                                    | `origin pr checks`              |
| `checkout [target]` | Fetch and check out the pull request branch                          | `origin pr checkout 13`         |
| `edit [target]`     | Edit the title, body, or base branch                                 | `origin pr edit -t "New title"` |
| `ready [target]`    | Mark a draft as ready for review                                     | `origin pr ready`               |
| `review [target]`   | Add a review                                                         | `origin pr review --approve`    |
| `comment [target]`  | Add a discussion comment                                             | `origin pr comment -b "LGTM"`   |
| `thread`            | List, resolve, reopen, and reply to review threads                   | `origin pr thread list`         |
| `merge [target]`    | Merge a pull request                                                 | `origin pr merge`               |
| `close [target]`    | Close a pull request                                                 | `origin pr close`               |
| `reopen [target]`   | Reopen a closed pull request                                         | `origin pr reopen`              |
| `refresh [target]`  | Snapshot a new version from the current refs                         | `origin pr refresh`             |

Pushing to the head branch snapshots a new version on its own. Run `origin pr refresh` only when `origin pr view` or `origin pr checks` still report the previous head commit. It does nothing when the resolved commits already match the latest version.

`origin pr review` files a formal review. `origin pr comment` adds a discussion comment. `origin pr view --comments` lists both.

## Create

| Option                   | Description                                                                    |
| ------------------------ | ------------------------------------------------------------------------------ |
| `-t, --title <title>`    | Title of the pull request                                                      |
| `-b, --body <body>`      | Body of the pull request                                                       |
| `--description <text>`   | Description of the pull request                                                |
| `-F, --body-file <path>` | Read the body from a file. Use `-` for stdin                                   |
| `-H, --head <branch>`    | Branch that holds your commits (default: current branch)                       |
| `-B, --base <branch>`    | Branch you want your code merged into (default: the repo default branch)       |
| `--push`                 | Push the head branch without prompting when it isn't on the remote yet         |
| `--remote <remote>`      | Remote to push to when the head branch isn't on the remote (default: `origin`) |
| `--status <status>`      | Initial status: `draft` or `open` (default: `draft`)                           |
| `-d, --draft`            | Create as a draft. Alias for `--status draft`                                  |
| `-f, --fill`             | Use commit information for the title and body                                  |
| `--fill-first`           | Use the first commit's information for the title and body                      |

## List and status

| Command  | Option                  | Description                                                                      |
| -------- | ----------------------- | -------------------------------------------------------------------------------- |
| `list`   | `-s, --state <state>`   | Filter by state: `open`, `closed`, `merged`, `draft`, or `all` (default: `open`) |
| `list`   | `--mine`                | Only show pull requests you authored                                             |
| `list`   | `-B, --base <branch>`   | Filter by base branch                                                            |
| `list`   | `-H, --head <branch>`   | Filter by head branch                                                            |
| `list`   | `--file <path>`         | Filter by a changed path. Repeatable; a trailing `/` matches a folder prefix     |
| `list`   | `-L, --limit <number>`  | Maximum number of items to fetch (default: `30`)                                 |
| `list`   | `--json <fields>`       | Output JSON with the fields you list, for example `number,title,status`          |
| `list`   | `-q, --jq <expression>` | Filter the JSON output with a jq expression. Requires `--json`                   |
| `status` | `-c, --conflict-status` | Show the merge conflict status of each pull request                              |

## View, diff, and checks

| Command  | Option                      | Description                                                          |
| -------- | --------------------------- | -------------------------------------------------------------------- |
| `view`   | `--diff`                    | Show the collected patch diff for the selected version               |
| `view`   | `--checks`                  | Include the mergeability and CI summary                              |
| `view`   | `-c, --comments`            | Show comments and reviews, with per-thread resolution state          |
| `view`   | `--name-only`               | With `--diff`, print only the changed file paths                     |
| `view`   | `--exclude <glob>`          | Exclude paths matching a glob. Repeatable                            |
| `view`   | `-w, --web`                 | Open the pull request in your browser                                |
| `view`   | `--change-version <number>` | Version to display (default: latest)                                 |
| `view`   | `--json <fields>`           | Output JSON with the fields you list. Bare `--json` lists the fields |
| `view`   | `-q, --jq <expression>`     | Filter the JSON output with a jq expression. Requires `--json`       |
| `diff`   | `--patch`                   | Display the diff in patch format                                     |
| `diff`   | `--name-only`               | Print only the changed file paths                                    |
| `diff`   | `-e, --exclude <glob>`      | Exclude files matching a glob. Repeatable                            |
| `checks` | `--watch`                   | Watch the checks until they finish                                   |
| `checks` | `--json <fields>`           | Output JSON with the fields you list                                 |
| `checks` | `-q, --jq <expression>`     | Filter the JSON output with a jq expression. Requires `--json`       |

`list`, `status`, `view`, `diff`, and `checks` also take `--color <always|never|auto>` (default: `auto`).

`--diff`, `--checks`, and `--comments` only shape the plain-text output of `origin pr view`. For JSON, request the fields you want instead, such as `files`, `mergeability`, `ciState`, `comments`, or `threads`.

## Checkout, edit, and lifecycle

| Command    | Option                   | Description                                                     |
| ---------- | ------------------------ | --------------------------------------------------------------- |
| `checkout` | `-b, --branch <name>`    | Local branch name to use (default: the pull request's head ref) |
| `checkout` | `--detach`               | Check out with a detached HEAD                                  |
| `checkout` | `-f, --force`            | Reset the existing local branch to the latest state             |
| `checkout` | `--remote <remote>`      | Remote to fetch from (default: `origin`)                        |
| `edit`     | `-t, --title <title>`    | Set a new title                                                 |
| `edit`     | `-b, --body <body>`      | Set a new body                                                  |
| `edit`     | `-F, --body-file <path>` | Read the new body from a file. Use `-` for stdin                |
| `edit`     | `-B, --base <branch>`    | Change the base branch                                          |
| `ready`    | `--undo`                 | Convert the pull request back to a draft                        |
| `merge`    | `--auto`                 | Merge once the requirements are met, then return                |
| `merge`    | `--disable-auto`         | Turn off merge-when-ready for this pull request                 |
| `close`    | `-c, --comment <body>`   | Leave a closing comment                                         |
| `reopen`   | `-c, --comment <body>`   | Add a reopening comment                                         |

## Review and comment

| Command   | Option                      | Description                                          |
| --------- | --------------------------- | ---------------------------------------------------- |
| `review`  | `-a, --approve`             | Approve the pull request                             |
| `review`  | `-c, --comment`             | File the review as a comment                         |
| `review`  | `-b, --body <body>`         | Review body text                                     |
| `review`  | `-F, --body-file <path>`    | Read the review body from a file. Use `-` for stdin  |
| `review`  | `--change-version <number>` | Version being reviewed (default: latest)             |
| `comment` | `-b, --body <body>`         | Comment body text                                    |
| `comment` | `-F, --body-file <path>`    | Read the comment body from a file. Use `-` for stdin |

## Review threads

Every review comment belongs to a thread with its own resolution state.

| Subcommand                            | Description                             | Usage                                     |
| ------------------------------------- | --------------------------------------- | ----------------------------------------- |
| `thread list [target]`                | List threads and their resolution state | `origin pr thread list --unresolved`      |
| `thread resolve <thread-id> [target]` | Resolve a thread                        | `origin pr thread resolve t_123`          |
| `thread reopen <thread-id> [target]`  | Reopen a resolved thread                | `origin pr thread reopen t_123`           |
| `thread reply <thread-id> [target]`   | Add a comment to a thread               | `origin pr thread reply t_123 -b "Fixed"` |

| Command | Option                      | Description                                                          |
| ------- | --------------------------- | -------------------------------------------------------------------- |
| `list`  | `--unresolved`              | Only show unresolved threads                                         |
| `list`  | `-c, --comments`            | Print every comment body instead of the opening excerpt              |
| `list`  | `--change-version <number>` | Only threads filed against this version (default: every version)     |
| `list`  | `--json <fields>`           | Output JSON with the fields you list, for example `id,resolved,path` |
| `list`  | `-q, --jq <expression>`     | Filter the JSON output with a jq expression. Requires `--json`       |
| `reply` | `-b, --body <body>`         | Reply body text                                                      |
| `reply` | `-F, --body-file <path>`    | Read the reply body from a file. Use `-` for stdin                   |

Scripts that gate on "all threads resolved" can use `origin pr thread list --unresolved --json id --jq length`.

## Getting help

Every subcommand takes `--help`:

```bash
origin pr --help
origin pr create --help
origin pr thread --help
```

## Next steps

- [Commands](https://cursor.com/docs/origin/cli/reference/commands.md)
- [Pull requests](https://cursor.com/docs/origin/pull-requests.md)
- [Clone, Push & Pull](https://cursor.com/docs/origin/git.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
