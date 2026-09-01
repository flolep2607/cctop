# gt CLI

## Getting started

Before you use the `gt` CLI in a repository, you'll need to install it, authenticate, and initialize your repo. After that, `gt` knows which branch is the trunk and can start tracking your stack relationships.

### Install the CLI

Install the `gt` CLI with Homebrew or npm:

```bash
# Homebrew
brew install withgraphite/tap/graphite
# npm
npm install -g @withgraphite/graphite-cli@stable
```

### Authenticate

1. Sign in to [review.cursor.com](https://review.cursor.com) with your GitHub account
2. Open **Settings > CLI** and create an auth token
3. Copy the `gt auth --token <your_cli_auth_token>` command
4. Paste it into your terminal and run it

### Initialize your repository

From inside the repository, run:

```bash
gt init
```

This sets the trunk branch and lets `gt` start tracking your stack.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
