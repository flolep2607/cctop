# Commands

Origin is currently released in early beta. You can create repos, push and pull with git, mirror from GitHub, browse and search code, open and merge pull requests, and share with your Cursor team.

Please submit any and all feedback to [hi@cursor.com](mailto:hi@cursor.com) to help us make the product better.

Every `origin` command with its options, except pull requests, which have their own page: [Pull request commands](https://cursor.com/docs/origin/cli/reference/pull-requests.md). To see the same information in your terminal, run `origin --help` for the command list or `origin <group> --help` for one group. To install the CLI, see [Install the CLI](https://cursor.com/docs/origin/cli.md).

The Origin CLI (`origin`) is separate from the Cursor Agent CLI (`agent`) documented under [CLI](https://cursor.com/docs/cli/overview.md).

## Global options

These work with any command:

| Option         | Description                                                                   |
| -------------- | ----------------------------------------------------------------------------- |
| `--auth-token` | Cursor auth token, used directly as a bearer token (also `CURSOR_AUTH_TOKEN`) |
| `--endpoint`   | API server endpoint (default: `https://origin.cursor.com`)                    |
| `--version`    | Show the installed version                                                    |
| `--help`       | Show help for the current command                                             |

## Command groups

| Command                                                               | Description                                                                        | Usage                              |
| --------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------- |
| `auth`                                                                | Sign in, sign out, and configure git authentication                                | `origin auth login`                |
| `repo`                                                                | Create, list, view, clone, and delete repositories                                 | `origin repo create my-project`    |
| [`pr`](https://cursor.com/docs/origin/cli/reference/pull-requests.md) | Create, review, and merge pull requests                                            | `origin pr create`                 |
| `ruleset`                                                             | View Origin rulesets (merge-time and push-time). Alias: `rs`                       | `origin ruleset list`              |
| `ssh-key`                                                             | Manage SSH keys registered with your Origin account                                | `origin ssh-key list`              |
| `api <endpoint>`                                                      | Make an authenticated request to the Origin REST API at `api.cursor.com/v1/origin` | `origin api /repos/{owner}/{repo}` |
| `completion`                                                          | Print a bash or zsh tab-completion script                                          | `origin completion >> ~/.zshrc`    |
| `update`                                                              | Update `origin` to the current release on your channel                             | `origin update`                    |
| `config`                                                              | Manage configuration, including the release channel                                | `origin config get-channel`        |

## Targeting a repository

Most commands infer the repository from the `origin` git remote in your current checkout. Override it with `-R, --repo` in `org/name` format:

```bash
origin pr list -R acme/checkout
```

## Authentication

Manage your session and the git credential helper.

| Subcommand  | Description                                               | Usage                   |
| ----------- | --------------------------------------------------------- | ----------------------- |
| `login`     | Sign in through the browser and configure git             | `origin auth login`     |
| `setup-git` | Reconfigure the git credential helper for the Origin host | `origin auth setup-git` |
| `status`    | Show the current authentication method and account        | `origin auth status`    |
| `logout`    | Clear stored credentials                                  | `origin auth logout`    |

| Command     | Option            | Description                                                                                                                     |
| ----------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `login`     | `--api-key <key>` | Sign in with an API key instead of the browser flow. When `CURSOR_API_KEY` is set, `origin auth login` uses it without the flag |
| `login`     | `--local`         | Configure the credential helper in this repository's git config instead of globally                                             |
| `setup-git` | `--host <host>`   | Origin git host to configure (default: derived from `--endpoint` / `CURSOR_ORIGIN_ENDPOINT`)                                    |
| `setup-git` | `--global`        | Apply to the global git config (default: `true`)                                                                                |
| `setup-git` | `--local`         | Apply to this repository's git config instead of the global one                                                                 |

`origin auth login` also installs the git credential helper, so `git push` and `git pull` against Origin remotes work without further setup.

## Repositories

| Subcommand                      | Description                                   | Usage                                       |
| ------------------------------- | --------------------------------------------- | ------------------------------------------- |
| `create <repo>`                 | Create a repository                           | `origin repo create acme/checkout`          |
| `create-mirrored <github-repo>` | Create a Cursor mirror of a GitHub repository | `origin repo create-mirrored acme/checkout` |
| `list`                          | List the repositories your account can access | `origin repo list`                          |
| `view [repo]`                   | Display a repository                          | `origin repo view acme/checkout`            |
| `clone <repo> [directory]`      | Clone over HTTPS using your saved login       | `origin repo clone acme/checkout`           |
| `delete <repo>`                 | Delete a repository                           | `origin repo delete acme/checkout`          |

Pass `org/name` to target a repository. `origin repo create <name>` without a slash creates the repository in your account's namespace.

| Command           | Option                          | Description                                                                |
| ----------------- | ------------------------------- | -------------------------------------------------------------------------- |
| `create`          | `--default-branch <branch>`     | Default branch for the new repository (server default: `main`)             |
| `create-mirrored` | `--namespace <namespace>`       | Namespace to create the mirror under (default: your account's namespace)   |
| `create-mirrored` | `--github-enterprise-id <uuid>` | GitHub Enterprise Server app UUID. Omit it when mirroring from github.com  |
| `list`            | `--namespace <namespace>`       | List one namespace instead of every namespace you can access               |
| `view`            | `--json <fields>`               | Output JSON with the fields you list, for example `org,name,defaultBranch` |
| `delete`          | `-y, --yes`                     | Skip the confirmation prompt. Required in a non-interactive shell          |

## Rulesets

View the merge-time and push-time rulesets configured for a repository:

| Subcommand          | Description                                               | Usage                      |
| ------------------- | --------------------------------------------------------- | -------------------------- |
| `list`              | List rulesets configured for a repository                 | `origin ruleset list`      |
| `view <ruleset-id>` | View a ruleset by ID, as printed by `origin ruleset list` | `origin ruleset view <id>` |

## SSH keys

Manage the SSH public keys on your Origin account:

| Subcommand       | Description                                                  | Usage                                      |
| ---------------- | ------------------------------------------------------------ | ------------------------------------------ |
| `add [key-file]` | Add an SSH public key to your account. `-t` names the key    | `origin ssh-key add ~/.ssh/id_ed25519.pub` |
| `list`           | List the SSH public keys on your account (`--json` for JSON) | `origin ssh-key list`                      |
| `delete <id>`    | Delete a key by ID, as printed by `origin ssh-key list`      | `origin ssh-key delete <id>`               |

## API requests

`origin api` makes an authenticated request to the Origin REST API at `api.cursor.com/v1/origin` for anything without a first-class command. It supports request flags similar to `gh api`: `-X, --method`, `-H, --header`, `-F, --field`, `-f, --raw-field`, `--input`, and `-q, --jq`. `{owner}`, `{repo}`, and `{branch}` placeholders expand from `-R, --repo`, the `ORIGIN_REPO` environment variable, or the `origin` git remote. See the [Origin API](https://cursor.com/docs/api/origin.md) docs for endpoints, authentication, and examples.

## Updates

Update to the current release on your channel:

```bash
origin update
```

The CLI updates from the `stable` channel by default. Switch with `origin config set-channel <latest|stable>` and check with `origin config get-channel`.

## Shell completion

Append the completion script to your shell config:

```bash
origin completion >> ~/.zshrc
```

Branch-valued flags such as `--head` and `--base` then complete local branch names.

## Environment variables

| Variable                 | Description                                                                               |
| ------------------------ | ----------------------------------------------------------------------------------------- |
| `CURSOR_API_KEY`         | API key for `origin auth login`. When set, login skips the browser flow                   |
| `CURSOR_AUTH_TOKEN`      | Bearer token, the same value as `--auth-token`                                            |
| `CURSOR_ORIGIN_ENDPOINT` | Default API server endpoint, the same value as `--endpoint`                               |
| `ORIGIN_REPO`            | Default repository in `org/name` format for `origin api` when there is no `origin` remote |
| `NO_COLOR`               | Turn off colored output                                                                   |

## Getting help

Every command takes `--help`:

```bash
origin --help
origin repo --help
origin repo create --help
```

## Next steps

- [Pull request commands](https://cursor.com/docs/origin/cli/reference/pull-requests.md)
- [Install the CLI](https://cursor.com/docs/origin/cli.md)
- [Create a repository](https://cursor.com/docs/origin/create-repository.md)
- [Clone, Push & Pull](https://cursor.com/docs/origin/git.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
