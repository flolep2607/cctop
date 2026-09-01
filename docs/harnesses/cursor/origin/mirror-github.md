# Mirror a GitHub repository

Origin is currently released in early beta. You can create repos, push and pull with git, mirror from GitHub, browse and search code, open and merge pull requests, and share with your Cursor team.

Please submit any and all feedback to [hi@cursor.com](mailto:hi@cursor.com) to help us make the product better.

Mirroring copies a GitHub repository into Origin and keeps Origin updated as the GitHub repo changes. Use it when the code already lives on GitHub and you want Origin browse, search, and agent workflows on that history.

## Prerequisites

- A Cursor account with Origin access on a Pro, Teams, or Enterprise plan
- The [Cursor GitHub app](https://cursor.com/docs/integrations/github.md) connected to the org or account that owns the repo
- GitHub admin access on the repository you want to sync (required to enable the mirror)

## Sync a repo

1. Open [cursor.com/codebase](https://cursor.com/codebase)
2. Select **Sync from GitHub**
3. Choose the GitHub organization and repository
4. Confirm the sync

When the sync finishes, open the Origin copy to browse files and commit history.

[Media](/docs-static/images/origin/sync-from-github.mp4)

You can confirm sync status later under the repository **Settings → General** tab. Synced repos show Origin as the mirror and GitHub as the source.

## What syncs

| Included                                     | Not included                         |
| -------------------------------------------- | ------------------------------------ |
| Git history, branches, and tags              | GitHub Issues                        |
| Code you can browse and search on Origin     | GitHub Actions workflows and secrets |
| Pull requests, which sync in both directions |                                      |
| Ongoing updates so Origin stays fresh        |                                      |

Pull requests on a mirrored repo work on Origin and sync back to GitHub. Issues and CI configuration stay on GitHub unless you rebuild them elsewhere.

## After you mirror

- Browse and search at [cursor.com/codebase](https://cursor.com/codebase)
- Clone the Origin remote from the green **Code** button if you want a local checkout from Origin. You can push too: pushes to a synced repo pass through to GitHub, which remains the source of truth.
- Attach cloud agents to the Origin repo
- Review [pull requests](https://cursor.com/docs/origin/pull-requests.md) on Origin, with changes syncing back to GitHub

## Detach from GitHub

To stop syncing, open **Settings → General** and select **Detach from GitHub** under **Danger Zone**. This stops the sync and converts the Origin copy into a standalone Origin-hosted repository: Origin becomes the source of truth, and pushes to the Origin remote no longer flow to GitHub. Your GitHub repository is not affected.

## Sync lag

If browse looks stale:

- Confirm the GitHub app still has access ([GitHub integration](https://cursor.com/docs/integrations/github.md))
- Confirm you are a GitHub admin on the source repo
- Re-run **Sync from GitHub** from [cursor.com/codebase](https://cursor.com/codebase), or check **Settings → General** for sync status

## When not to mirror

If you only want automated review comments on GitHub PRs, [Cursor Review](https://cursor.com/docs/cursor-review/overview.md) and [Bugbot](https://cursor.com/docs/bugbot.md) do that without moving storage. Mirror when you want Origin-hosted code storage, browse, and pull requests.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
