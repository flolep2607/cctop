# Install the CLI

Origin is currently released in early beta. You can create repos, push and pull with git, mirror from GitHub, browse and search code, open and merge pull requests, and share with your Cursor team.

Please submit any and all feedback to [hi@cursor.com](mailto:hi@cursor.com) to help us make the product better.

The Origin CLI (`origin`) is separate from the Cursor Agent CLI (`agent`) documented under [CLI](https://cursor.com/docs/cli/overview.md).

## macOS, Linux and Windows (WSL)

Install the Origin CLI with a single command:

```bash
curl -fsSL https://downloads.cursor.com/origin/install.sh | sh
```

The installer puts the binary at `~/.local/bin/origin`. If your shell says `command not found: origin` (for example `zsh: command not found: origin`), add that directory to your `PATH`.

For zsh:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

For bash:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

## Verification

After installation, verify that the Origin CLI is working correctly:

```bash
origin --version
```

## Sign in

Sign in to your Cursor account:

```bash
origin auth login
```

Complete the browser flow with the Cursor account that has Origin access.

Signing in also sets up the git credential helper, so `git push` and `git pull` against Origin remotes work without further setup.

## Manage repositories

The CLI can create and delete Origin repositories, so you do not have to open the web UI:

```bash
origin repo create my-project
origin repo delete acme/my-project
```

`origin repo create` without a slash creates the repository in your account's namespace. `origin repo delete` always takes the full `org/name`.

Cursor agents can drive the same commands. Ask the agent to store a project on Origin and it can install the CLI, sign in, create the repo, set the remote, and push.

For every command and flag, see the [command reference](https://cursor.com/docs/origin/cli/reference/commands.md) and [pull request commands](https://cursor.com/docs/origin/cli/reference/pull-requests.md).

## Updates

To update the Origin CLI to the current release:

```bash
origin update
```

## Next steps

- [Commands](https://cursor.com/docs/origin/cli/reference/commands.md)
- [Pull request commands](https://cursor.com/docs/origin/cli/reference/pull-requests.md)
- [Create a repository](https://cursor.com/docs/origin/create-repository.md)
- [Clone, Push & Pull](https://cursor.com/docs/origin/git.md)
- [Mirror a GitHub repo](https://cursor.com/docs/origin/mirror-github.md)
- [Browse & Search](https://cursor.com/docs/origin/browse.md)
- [Integrations](https://cursor.com/docs/origin/integrations.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
