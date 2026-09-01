# Clone, Push & Pull

Origin is currently released in early beta. You can create repos, push and pull with git, mirror from GitHub, browse and search code, open and merge pull requests, and share with your Cursor team.

Please submit any and all feedback to [hi@cursor.com](mailto:hi@cursor.com) to help us make the product better.

Origin works with standard git. Open a repo at [cursor.com/codebase](https://cursor.com/codebase), select the green **Code** dropdown, and copy the clone URL.

## Clone with HTTPS

The **Code** menu includes an **HTTPS** tab:

```text
https://origin.cursor.com/{owner}/{repo}.git
```

Example:

```bash
git clone https://origin.cursor.com/acme/checkout.git
```

## Clone with the Origin CLI

The same menu has an **Origin CLI** tab with CLI-oriented setup. Install and sign in first:

```bash
curl -fsSL https://downloads.cursor.com/origin/install.sh | sh
origin auth login
```

See [Install the Origin CLI](https://cursor.com/docs/origin/cli.md).

## Authenticate

Sign in with the Origin CLI before the first git operation if you have not already:

```bash
origin auth login
```

Then clone, fetch, pull, or push with git.

## Add a remote to an existing repo

```bash
git remote add origin https://origin.cursor.com/{owner}/{repo}.git
git push -u origin main
```

To keep GitHub and Origin in parallel while you evaluate:

```bash
git remote set-url --add --push origin git@github.com:acme/checkout.git
git remote set-url --add --push origin https://origin.cursor.com/acme/checkout.git
```

For a full history copy from GitHub into Origin, prefer [mirroring](https://cursor.com/docs/origin/mirror-github.md).

## Pull latest

```bash
git pull origin main
```

## Troubleshooting

If clone or push fails, confirm you are signed in with `origin auth login`. See [Install the Origin CLI](https://cursor.com/docs/origin/cli.md).

If your shell says `command not found: origin` after install, add `~/.local/bin` to your `PATH` (for zsh: append `export PATH="$HOME/.local/bin:$PATH"` to `~/.zshrc`, then `source ~/.zshrc`). Details are on the CLI page.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
